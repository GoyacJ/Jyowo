import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { TaskProjection, TimelineItemProjection } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'

import { TaskWorkbenchSummary } from './TaskWorkbenchSummary'

describe('TaskWorkbenchSummary', () => {
  beforeEach(() => {
    uiStore.setState({ taskWorkbenchByTaskId: {}, taskWorkbenchSummaryCollapsed: false })
  })

  it('opens the selected summary target and marks the active row', () => {
    const onOpen = vi.fn()
    const { rerender } = renderSummary(onOpen)

    const changes = screen.getByRole('button', { name: /Changes/ })
    fireEvent.click(changes)
    expect(onOpen).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: 'diff',
        title: '3 files changed, 12 insertions, 4 deletions',
      }),
      changes,
    )

    act(() => uiStore.getState().openTaskWorkbench(onOpen.mock.calls[0]?.[0]))
    rerender(summary(onOpen))
    expect(screen.getByRole('button', { name: /Changes/ })).toHaveAttribute('aria-current', 'true')
    expect(screen.getByRole('button', { name: /demo\.mp4/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /\+12-4/ })).toBeInTheDocument()
  })

  it('collapses without discarding derived task context', () => {
    renderSummary(vi.fn())
    fireEvent.click(screen.getByRole('button', { name: 'Collapse task context' }))

    expect(screen.queryByRole('button', { name: /Changes/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Expand task context' })).toBeInTheDocument()
    expect(uiStore.getState().taskWorkbenchSummaryCollapsed).toBe(true)
  })

  it('uses explicit partial-failure labels without adding audit entries', () => {
    const mixedProjection: TaskProjection = {
      ...projection,
      subagents: [
        subagent('running', 'child-running'),
        subagent('failed', 'child-failed'),
        subagent('completed', 'child-complete'),
      ],
    }
    render(
      <TaskWorkbenchSummary
        events={[]}
        onOpen={vi.fn()}
        projection={mixedProjection}
        timeline={timeline}
      />,
      {
        wrapper: ({ children }) => (
          <I18nextProvider i18n={createAppI18n('en-US')}>{children}</I18nextProvider>
        ),
      },
    )

    expect(screen.getByRole('button', { name: /1 running · 1\/3 failed/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /historical issue/ })).not.toBeInTheDocument()
  })

  it('keeps task context available in a keyboard-accessible narrow-screen drawer', async () => {
    const onOpen = vi.fn()
    render(
      <TaskWorkbenchSummary
        events={[]}
        mobile
        onOpen={onOpen}
        projection={{
          ...projection,
          workspace: { mode: 'current', root: '/repo/Jyowo' },
        }}
        timeline={timeline}
      />,
      {
        wrapper: ({ children }) => (
          <I18nextProvider i18n={createAppI18n('en-US')}>{children}</I18nextProvider>
        ),
      },
    )

    const trigger = screen.getByRole('button', { name: 'Expand task context' })
    fireEvent.click(trigger)
    const dialog = screen.getByRole('dialog', { name: 'Task context' })
    expect(dialog).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close task context' })).toHaveFocus()

    fireEvent.keyDown(dialog, { key: 'Escape' })
    await waitFor(() => expect(trigger).toHaveFocus())
    expect(screen.queryByRole('dialog', { name: 'Task context' })).not.toBeInTheDocument()

    fireEvent.click(trigger)
    const environment = screen.getByRole('button', { name: /Local workspace/ })
    fireEvent.click(environment)
    expect(onOpen).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'environment', resourceId: 'workspace' }),
      environment,
    )
    expect(screen.queryByRole('dialog', { name: 'Task context' })).not.toBeInTheDocument()
  })
})

function renderSummary(onOpen: Parameters<typeof TaskWorkbenchSummary>[0]['onOpen']) {
  return render(summary(onOpen), {
    wrapper: ({ children }) => (
      <I18nextProvider i18n={createAppI18n('en-US')}>{children}</I18nextProvider>
    ),
  })
}

function summary(onOpen: Parameters<typeof TaskWorkbenchSummary>[0]['onOpen']) {
  return (
    <TaskWorkbenchSummary events={[]} onOpen={onOpen} projection={projection} timeline={timeline} />
  )
}

const taskId = '01J00000000000000000000001'
const projection: TaskProjection = {
  archived: false,
  lastGlobalOffset: 1,
  queue: [],
  state: 'completed',
  streamVersion: 1,
  taskId,
  title: 'Workbench redesign',
}
const timeline: TimelineItemProjection[] = [
  {
    blobId: 'diff-blob',
    globalOffset: 1,
    id: 'diff-event',
    incomplete: false,
    kind: 'diff',
    summary: '3 files changed, 12 insertions, 4 deletions',
  },
  {
    blobId: 'artifact-blob',
    globalOffset: 2,
    id: 'artifact-event',
    incomplete: false,
    kind: 'artifact',
    summary: 'demo.mp4',
  },
]

function subagent(state: 'completed' | 'failed' | 'running', childTaskId: string) {
  return {
    actorId: `actor-${childTaskId}`,
    childTaskId,
    contextCursor: 1,
    delegationId: `delegation-${childTaskId}`,
    detached: false,
    parentSegmentId: 'parent-segment',
    parentTaskId: taskId,
    segmentId: `segment-${childTaskId}`,
    startedAt: '2026-07-14T00:00:00Z',
    state,
    summary: childTaskId,
  }
}
