import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { TaskEventEnvelope } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { AuditPanel } from './AuditPanel'

const taskId = '00000000000000000000000001'

describe('AuditPanel', () => {
  it('loads task-scoped audit history on demand and walks the backward cursor', async () => {
    const latest = event(20, 'engine.run_ended')
    const older = event(4, 'task.created')
    const loadTaskEvents = vi
      .fn()
      .mockResolvedValueOnce({ events: [latest], nextBeforeOffset: 20, taskId })
      .mockResolvedValueOnce({ events: [older], nextBeforeOffset: null, taskId })

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={20}
        taskId={taskId}
        timeline={[]}
      />,
    )

    expect(await screen.findByText('engine.run_ended')).toBeInTheDocument()
    expect(loadTaskEvents).toHaveBeenLastCalledWith(taskId, undefined)

    fireEvent.click(screen.getByRole('button', { name: 'Older events' }))

    expect(await screen.findByText('task.created')).toBeInTheDocument()
    expect(loadTaskEvents).toHaveBeenLastCalledWith(taskId, 20)
    expect(screen.getByRole('button', { name: 'Newer events' })).toBeEnabled()
    expect(screen.queryByRole('button', { name: 'Older events' })).not.toBeInTheDocument()
  })

  it('bounds the newest audit view while including committed live events', async () => {
    const loadTaskEvents = vi.fn().mockResolvedValue({
      events: Array.from({ length: 16 }, (_, index) => event(index + 1, `stored.${index + 1}`)),
      nextBeforeOffset: 1,
      taskId,
    })
    const liveEvents = Array.from({ length: 20 }, (_, index) =>
      event(17 + index, `live.${index + 1}`),
    )

    const { container } = renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={liveEvents}
        snapshotOffset={16}
        taskId={taskId}
        timeline={[]}
      />,
    )

    await waitFor(() => expect(loadTaskEvents).toHaveBeenCalledOnce())
    expect(container.querySelectorAll('[data-audit-event]').length).toBeLessThanOrEqual(16)
    expect(screen.getByText('live.20')).toBeInTheDocument()
  })

  it('continues paging from the oldest visible event after live events fill the newest page', async () => {
    const storedEvents = Array.from({ length: 16 }, (_, index) =>
      event(100 - index, `stored.${100 - index}`),
    )
    const liveEvents = Array.from({ length: 5 }, (_, index) =>
      event(101 + index, `live.${101 + index}`),
    )
    const loadTaskEvents = vi
      .fn()
      .mockResolvedValueOnce({ events: storedEvents, nextBeforeOffset: 85, taskId })
      .mockResolvedValueOnce({ events: [event(89, 'stored.89')], nextBeforeOffset: null, taskId })

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={liveEvents}
        snapshotOffset={100}
        taskId={taskId}
        timeline={[]}
      />,
    )

    expect(await screen.findByText('live.105')).toBeInTheDocument()
    expect(screen.getByText('stored.90')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Older events' }))

    await waitFor(() => expect(loadTaskEvents).toHaveBeenLastCalledWith(taskId, 90))
    expect(await screen.findByText('stored.89')).toBeInTheDocument()
  })

  it('reloads the newest page when a newer snapshot replaces live events', async () => {
    const loadTaskEvents = vi
      .fn()
      .mockResolvedValueOnce({
        events: [event(20, 'stored.before-refresh')],
        nextBeforeOffset: null,
        taskId,
      })
      .mockResolvedValueOnce({
        events: [event(30, 'stored.after-refresh')],
        nextBeforeOffset: null,
        taskId,
      })
    const { rerender } = renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[event(21, 'live.before-refresh')]}
        snapshotOffset={20}
        taskId={taskId}
        timeline={[]}
      />,
    )

    expect(await screen.findByText('live.before-refresh')).toBeInTheDocument()
    rerender(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={30}
        taskId={taskId}
        timeline={[]}
      />,
    )

    await waitFor(() => expect(loadTaskEvents).toHaveBeenCalledTimes(2))
    expect(await screen.findByText('stored.after-refresh')).toBeInTheDocument()
  })

  it('disables pagination while the next audit page is loading', async () => {
    const loadTaskEvents = vi
      .fn()
      .mockResolvedValueOnce({
        events: [event(20, 'stored.latest')],
        nextBeforeOffset: 20,
        taskId,
      })
      .mockImplementationOnce(() => new Promise(() => {}))

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={20}
        taskId={taskId}
        timeline={[]}
      />,
    )

    const olderButton = await screen.findByRole('button', { name: 'Older events' })
    fireEvent.click(olderButton)
    await waitFor(() => expect(loadTaskEvents).toHaveBeenCalledTimes(2))

    expect(olderButton).toBeDisabled()
  })

  it('retries a failed audit request', async () => {
    const loadTaskEvents = vi
      .fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce({
        events: [event(20, 'stored.recovered')],
        nextBeforeOffset: null,
        taskId,
      })

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={20}
        taskId={taskId}
        timeline={[]}
      />,
    )

    expect(await screen.findByText('Audit events are unavailable')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('stored.recovered')).toBeInTheDocument()
    expect(loadTaskEvents).toHaveBeenCalledTimes(2)
  })

  it('shows localized object details for a selected audit event', async () => {
    const selected = {
      ...event(42, 'engine.tool_use_started'),
      payload: { command: 'pnpm test', tool: 'exec_command' },
    }
    const loadTaskEvents = vi.fn().mockResolvedValue({
      events: [selected],
      nextBeforeOffset: null,
      taskId,
    })

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={42}
        target={{
          kind: 'audit',
          resourceId: selected.eventId,
          sourceEventId: selected.eventId,
          taskId,
          title: 'Using exec_command',
        }}
        taskId={taskId}
        timeline={[
          {
            globalOffset: 42,
            id: selected.eventId,
            incomplete: true,
            kind: 'tool_activity',
            summary: 'Using exec_command',
          },
        ]}
      />,
    )

    expect(await screen.findByRole('heading', { name: 'Using exec_command' })).toBeInTheDocument()
    expect(screen.getByText('In progress')).toBeInTheDocument()
    expect(screen.getByText('supervisor')).toBeInTheDocument()
    expect(screen.getByText(/pnpm test/)).toBeInTheDocument()
    expect(screen.getByText('engine.tool_use_started')).toBeInTheDocument()
  })

  it('opens event details from the general audit list', async () => {
    const selected = event(43, 'run.completed')
    const loadTaskEvents = vi.fn().mockResolvedValue({
      events: [selected],
      nextBeforeOffset: null,
      taskId,
    })

    renderPanel(
      <AuditPanel
        client={{ loadTaskEvents }}
        liveEvents={[]}
        snapshotOffset={43}
        taskId={taskId}
        timeline={[]}
      />,
    )

    fireEvent.click(await screen.findByRole('button', { name: /run.completed/ }))
    expect(screen.getByRole('button', { name: 'Back to event list' })).toBeInTheDocument()
    expect(screen.getByText('Complete')).toBeInTheDocument()
  })
})

function renderPanel(ui: React.ReactNode) {
  const i18n = createAppI18n('en-US')
  return render(ui, {
    wrapper: ({ children }) => <I18nextProvider i18n={i18n}>{children}</I18nextProvider>,
  })
}

function event(globalOffset: number, eventType: string): TaskEventEnvelope {
  return {
    eventId: `0000000000000000000000${String(globalOffset).padStart(4, '0')}`,
    eventType,
    globalOffset,
    payload: {},
    recordedAt: '2026-07-12T01:00:00Z',
    schemaVersion: 1,
    source: { kind: 'supervisor' },
    streamSequence: globalOffset,
    taskId,
  }
}
