import '@testing-library/jest-dom/vitest'

import {
  act,
  fireEvent,
  screen,
  render as testingLibraryRender,
  waitFor,
  within,
} from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { TaskEventEnvelope, TaskProjection } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchTarget } from '@/shared/state/workbench-selection'

import { TaskWorkbench } from './TaskWorkbench'

function render(ui: React.ReactNode, locale: 'en-US' | 'zh-CN' = 'en-US') {
  return testingLibraryRender(ui, {
    wrapper: ({ children }) => (
      <I18nextProvider i18n={createAppI18n(locale)}>{children}</I18nextProvider>
    ),
  })
}

describe('TaskWorkbench', () => {
  beforeEach(() => {
    uiStore.setState({ taskWorkbenchByTaskId: {}, taskWorkbenchWidth: 400 })
    openTarget(target('diff', 'Changes', { blobId }))
  })

  it('loads the active object and exposes object-based tab semantics', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('diff --git a/a.rs b/a.rs\n+fixed'))

    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    const tab = screen.getByRole('tab', { name: 'Changes' })
    expect(tab).toHaveAttribute('aria-selected', 'true')
    expect(tab).toHaveAttribute('aria-controls')
    expect(screen.getByRole('tabpanel', { name: 'Changes' })).toBeInTheDocument()
    expect(await screen.findByText(/diff --git/)).toBeInTheDocument()
    expect(readBlob).toHaveBeenCalledOnce()
    expect(readBlob).toHaveBeenCalledWith(blobId)
  })

  it('reuses the preview tab and preserves pinned objects', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('artifact body'))
    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Keep tab open' }))
    act(() => openTarget(target('file', 'test-output.txt', { blobId: commandBlobId })))

    expect(screen.getAllByRole('tab')).toHaveLength(2)
    expect(screen.getByRole('tab', { name: 'Changes' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'test-output.txt' })).toHaveAttribute(
      'aria-selected',
      'true',
    )

    act(() => openTarget(target('source', 'Design source', { blobId: sourceBlobId })))
    expect(screen.getAllByRole('tab')).toHaveLength(2)
    expect(screen.queryByRole('tab', { name: 'test-output.txt' })).not.toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Design source' })).toBeInTheDocument()
  })

  it('supports keyboard tab switching and double-click pinning', () => {
    render(<TaskWorkbench client={workbenchClient()} events={events} projection={projection} />)
    fireEvent.doubleClick(screen.getByRole('tab', { name: 'Changes' }))
    act(() => openTarget(target('subagent', 'Subagents', { resourceId: 'all' })))

    const subagentTab = screen.getByRole('tab', { name: 'Subagents' })
    subagentTab.focus()
    fireEvent.keyDown(subagentTab, { key: 'ArrowLeft' })

    expect(screen.getByRole('tab', { name: 'Changes' })).toHaveAttribute('aria-selected', 'true')
  })

  it('renders file, subagent, and source targets', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('artifact body'))
    render(
      <TaskWorkbench
        client={workbenchClient(readBlob, [historicalAuditEvent])}
        events={events}
        projection={projection}
        timeline={historicalTimeline}
      />,
    )

    act(() => openTarget(target('file', 'command-output.txt', { blobId: commandBlobId })))
    expect(await screen.findByText('artifact body')).toBeInTheDocument()

    act(() => openTarget(target('subagent', 'Subagents', { resourceId: 'all' })))
    expect(screen.getByText('Reviewing recovery')).toBeInTheDocument()

    act(() => openTarget(target('source', 'design.png')))
    expect(
      within(screen.getByRole('tabpanel', { name: 'design.png' })).getByText('design.png'),
    ).toBeInTheDocument()
  })

  it('shows missing resources and retries transient failures', async () => {
    const readBlob = vi
      .fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(blob('recovered output'))
    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    expect(await screen.findByText('The resource could not be loaded.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('recovered output')).toBeInTheDocument()

    readBlob.mockResolvedValueOnce({ ...blob(''), bytes: null, missing: true })
    act(() => openTarget(target('artifact', 'Missing output', { blobId: commandBlobId })))
    expect(await screen.findByText('Artifact is unavailable')).toBeInTheDocument()
  })

  it('locates the source event and closes back to the opener contract', () => {
    const onClosed = vi.fn()
    const onLocateInTimeline = vi.fn()
    render(
      <TaskWorkbench
        client={workbenchClient()}
        events={events}
        onClosed={onClosed}
        onLocateInTimeline={onLocateInTimeline}
        projection={projection}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Locate in conversation' }))
    expect(onLocateInTimeline).toHaveBeenCalledWith(eventId)

    fireEvent.click(screen.getByRole('button', { name: 'Close task workbench' }))
    expect(onClosed).toHaveBeenCalledOnce()
    expect(uiStore.getState().taskWorkbenchByTaskId[taskId]?.open).toBe(false)
  })

  it('localizes controls, target kinds, and subagent state', () => {
    act(() => openTarget(target('subagent', '子智能体', { resourceId: 'all' })))
    render(
      <TaskWorkbench client={workbenchClient()} events={events} projection={projection} />,
      'zh-CN',
    )

    expect(screen.getByRole('complementary', { name: '任务工作台' })).toBeInTheDocument()
    expect(screen.getByText(/子智能体 · Repair scheduler/)).toBeInTheDocument()
    expect(screen.getByText('运行中')).toBeInTheDocument()
    expect(screen.queryByText('running')).not.toBeInTheDocument()
  })

  it('uses the persisted width variable and hides after the last tab closes', async () => {
    uiStore.getState().setTaskWorkbenchWidth(520)
    render(<TaskWorkbench client={workbenchClient()} events={events} projection={projection} />)

    expect(screen.getByTestId('task-workbench')).toHaveStyle('--task-workbench-width: 520px')
    fireEvent.click(screen.getByRole('button', { name: 'Close Changes' }))
    await waitFor(() => expect(screen.queryByTestId('task-workbench')).not.toBeInTheDocument())
  })

  it('previews image blobs and revokes their object URLs', async () => {
    const createObjectURL = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:preview-image')
    const revokeObjectURL = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined)
    const readBlob = vi.fn().mockResolvedValue(blob('image-bytes', 'image/png'))
    act(() => openTarget(target('source', 'diagram.png', { blobId: sourceBlobId })))

    const { unmount } = render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    expect(await screen.findByRole('img', { name: 'diagram.png' })).toHaveAttribute(
      'src',
      'blob:preview-image',
    )
    expect(createObjectURL).toHaveBeenCalledOnce()
    unmount()
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:preview-image')
  })

  it('shows metadata instead of decoding unsupported binary blobs', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('binary', 'application/zip'))
    act(() => openTarget(target('artifact', 'bundle.zip', { blobId: sourceBlobId })))

    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    expect(
      await screen.findByText('Preview is not supported for this file type.'),
    ).toBeInTheDocument()
    expect(screen.getByText('application/zip · 6 bytes')).toBeInTheDocument()
    expect(screen.queryByText('binary')).not.toBeInTheDocument()
  })

  it('shows only the selected subagent', () => {
    const secondSubagent = {
      ...projection.subagents?.[0],
      actorId: '01J00000000000000000000015',
      childTaskId: '01J00000000000000000000016',
      delegationId: '01J00000000000000000000017',
      segmentId: '01J00000000000000000000018',
      summary: 'Second agent',
    } as NonNullable<TaskProjection['subagents']>[number]
    const selectedProjection = {
      ...projection,
      subagents: [...(projection.subagents ?? []), secondSubagent],
    }
    render(
      <TaskWorkbench
        client={workbenchClient()}
        events={events}
        projection={selectedProjection}
        timeline={historicalTimeline}
      />,
    )

    act(() =>
      openTarget(
        target('subagent', 'Selected agent', {
          resourceId: projection.subagents?.[0]?.childTaskId,
        }),
      ),
    )
    expect(screen.getByText('Reviewing recovery')).toBeInTheDocument()
    expect(screen.queryByText('Second agent')).not.toBeInTheDocument()
  })

  it('moves focus to the adjacent active tab when closing the current tab', async () => {
    render(<TaskWorkbench client={workbenchClient()} events={events} projection={projection} />)
    fireEvent.click(screen.getByRole('button', { name: 'Keep tab open' }))
    act(() => openTarget(target('file', 'command-output.txt', { blobId: commandBlobId })))

    fireEvent.click(screen.getByRole('button', { name: 'Close command-output.txt' }))

    await waitFor(() => expect(screen.getByRole('tab', { name: 'Changes' })).toHaveFocus())
    expect(screen.getByRole('tab', { name: 'Changes' })).toHaveAttribute('aria-selected', 'true')
  })
})

function openTarget(value: TaskWorkbenchTarget) {
  uiStore.getState().openTaskWorkbench(value)
}

function target(
  kind: TaskWorkbenchTarget['kind'],
  title: string,
  overrides: Partial<TaskWorkbenchTarget> = {},
): TaskWorkbenchTarget {
  return {
    kind,
    resourceId: overrides.blobId ?? `${kind}-resource`,
    sourceEventId: eventId,
    taskId,
    title,
    ...overrides,
  }
}

function blob(text: string, mediaType = 'text/plain') {
  return {
    blobId,
    bytes: new TextEncoder().encode(text),
    contentHash: Array.from({ length: 32 }, () => 1),
    mediaType,
    missing: false,
    size: text.length,
  }
}

function workbenchClient(readBlob = vi.fn().mockResolvedValue(blob('')), auditEvents = events) {
  return {
    loadTaskEvents: vi.fn().mockResolvedValue({
      events: auditEvents,
      nextBeforeOffset: null,
      taskId,
    }),
    readBlob,
    request: vi.fn(),
  }
}

const taskId = '01J00000000000000000000001'
const segmentId = '01J00000000000000000000002'
const eventId = '01J00000000000000000000003'
const blobId = '01J00000000000000000000004'
const commandBlobId = '01J00000000000000000000009'
const sourceBlobId = '01J00000000000000000000010'

const projection: TaskProjection = {
  archived: false,
  lastGlobalOffset: 3,
  queue: [],
  state: 'running',
  streamVersion: 3,
  subagents: [
    {
      actorId: '01J00000000000000000000005',
      childTaskId: '01J00000000000000000000006',
      contextCursor: 2,
      delegationId: '01J00000000000000000000007',
      detached: false,
      parentSegmentId: segmentId,
      parentTaskId: taskId,
      segmentId: '01J00000000000000000000008',
      startedAt: '2026-07-11T06:00:00Z',
      state: 'running',
      summary: 'Reviewing recovery',
    },
  ],
  taskId,
  title: 'Repair scheduler',
  workspace: { mode: 'current', root: '/workspace/recovery' },
}

const events: TaskEventEnvelope[] = [
  event(1, 'workspace.acquired'),
  event(2, 'permission.requested'),
  event(3, 'engine.tool_use_completed'),
]

const historicalAuditEvent = event(0, 'engine.run_started')

const historicalTimeline = [
  item(1, 'notice', 'Workspace acquired'),
  item(2, 'image', 'design.png'),
  item(3, 'error', 'Command failed'),
]

function item(globalOffset: number, kind: 'error' | 'image' | 'notice', summary: string) {
  return {
    globalOffset,
    id: `timeline-${globalOffset}`,
    incomplete: false,
    kind,
    summary,
  }
}

function event(globalOffset: number, eventType: string): TaskEventEnvelope {
  return {
    eventId: `01J00000000000000000000${String(globalOffset + 10).padStart(2, '0')}`,
    eventType,
    globalOffset,
    payload: { segmentId },
    recordedAt: '2026-07-11T06:00:00Z',
    schemaVersion: 1,
    source: { kind: 'engine' },
    streamSequence: globalOffset,
    taskId,
  }
}
