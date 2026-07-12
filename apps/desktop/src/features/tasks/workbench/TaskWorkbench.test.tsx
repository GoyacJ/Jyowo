import '@testing-library/jest-dom/vitest'

import {
  act,
  fireEvent,
  screen,
  render as testingLibraryRender,
  waitFor,
} from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { TaskEventEnvelope, TaskProjection } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { TaskWorkbenchSelection } from '@/shared/state/workbench-selection'

import { TaskWorkbench } from './TaskWorkbench'

function render(ui: React.ReactNode) {
  const i18n = createAppI18n('en-US')
  return testingLibraryRender(ui, {
    wrapper: ({ children }) => <I18nextProvider i18n={i18n}>{children}</I18nextProvider>,
  })
}

describe('TaskWorkbench', () => {
  beforeEach(() => {
    uiStore.setState({ taskWorkbenchMode: 'inspector', taskWorkbenchSelection: null })
  })

  it('opens the selected diff blob and preserves task, segment, and event identity', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('diff --git a/a.rs b/a.rs\n+fixed'))
    uiStore.setState({
      taskWorkbenchSelection: selection('changes', { blobId }),
    })

    const { rerender } = render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    expect(screen.getByRole('tab', { name: 'Changes' })).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByText(/diff --git/)).toBeInTheDocument()
    expect(readBlob).toHaveBeenCalledWith(blobId)
    expect(screen.getByText(taskId)).toBeInTheDocument()
    expect(screen.getByText(segmentId)).toBeInTheDocument()
    expect(screen.getByText(eventId)).toBeInTheDocument()

    rerender(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )
    expect(readBlob).toHaveBeenCalledOnce()
  })

  it('switches projection-driven command, agent, environment, source, and audit panels', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('artifact body'))
    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    await select('commands', { blobId })
    expect(await screen.findByText('artifact body')).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Commands' })).toHaveAttribute('aria-selected', 'true')

    await select('agents')
    expect(screen.getByText('Reviewing recovery')).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Agents' })).toHaveAttribute('aria-selected', 'true')

    await select('environment')
    expect(screen.getByText('workspace.acquired')).toBeInTheDocument()

    await select('sources', { blobId })
    expect(await screen.findByText('artifact body')).toBeInTheDocument()

    await select('audit')
    expect(screen.getByText('permission.requested')).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Audit' })).toHaveAttribute('aria-selected', 'true')
  })

  it('shows a missing blob instead of an empty artifact panel', async () => {
    uiStore.setState({ taskWorkbenchSelection: selection('commands', { blobId }) })
    render(
      <TaskWorkbench
        client={workbenchClient(
          vi.fn().mockResolvedValue({ ...blob(''), bytes: null, missing: true }),
        )}
        events={events}
        projection={projection}
      />,
    )

    expect(await screen.findByText('Artifact is unavailable')).toBeInTheDocument()
  })

  it('clears the selected artifact when switching to a different panel', async () => {
    const readBlob = vi.fn().mockResolvedValue(blob('diff --git a/a.rs b/a.rs\n+fixed'))
    uiStore.setState({ taskWorkbenchSelection: selection('changes', { blobId }) })
    render(
      <TaskWorkbench client={workbenchClient(readBlob)} events={events} projection={projection} />,
    )

    expect(await screen.findByText(/diff --git/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('tab', { name: 'Commands' }))

    expect(uiStore.getState().taskWorkbenchSelection?.blobId).toBeUndefined()
    expect(screen.queryByText(/diff --git/)).not.toBeInTheDocument()
    expect(screen.queryByText('Artifact is unavailable')).not.toBeInTheDocument()
  })

  it('clears incompatible event identity when switching panels', () => {
    uiStore.setState({ taskWorkbenchSelection: selection('commands', { blobId }) })
    render(
      <TaskWorkbench
        client={workbenchClient(vi.fn().mockResolvedValue(blob('output')))}
        events={events}
        projection={projection}
      />,
    )

    fireEvent.click(screen.getByRole('tab', { name: 'Changes' }))

    expect(uiStore.getState().taskWorkbenchSelection).toEqual({
      panel: 'changes',
      taskId,
    })
  })

  it('hides decorative workbench icons from assistive technology', () => {
    const { container } = render(
      <TaskWorkbench client={workbenchClient()} events={events} projection={projection} />,
    )

    expect(container.querySelectorAll('header svg')).not.toHaveLength(0)
    for (const icon of container.querySelectorAll('header svg')) {
      expect(icon).toHaveAttribute('aria-hidden', 'true')
    }
  })

  it('localizes workbench controls and empty state in Chinese', () => {
    render(
      <I18nextProvider i18n={createAppI18n('zh-CN')}>
        <TaskWorkbench client={workbenchClient()} events={[]} projection={projection} />
      </I18nextProvider>,
    )

    expect(screen.getByRole('complementary', { name: '任务工作台' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '更改' })).toBeInTheDocument()
    expect(screen.getByText('请选择一条更改事件以查看补丁。')).toBeInTheDocument()
    expect(screen.queryByText('Workbench')).not.toBeInTheDocument()
  })

  it('localizes subagent state in Chinese', async () => {
    render(
      <I18nextProvider i18n={createAppI18n('zh-CN')}>
        <TaskWorkbench client={workbenchClient()} events={[]} projection={projection} />
      </I18nextProvider>,
    )

    fireEvent.click(screen.getByRole('tab', { name: '代理' }))
    expect(screen.getByText('运行中')).toBeInTheDocument()
    expect(screen.queryByText('running')).not.toBeInTheDocument()
  })

  it('renders historical environment, source, and audit projections from the snapshot', async () => {
    render(
      <TaskWorkbench
        client={workbenchClient()}
        events={[]}
        projection={projection}
        timeline={historicalTimeline}
      />,
    )

    await select('environment')
    expect(screen.getByText('Workspace acquired')).toBeInTheDocument()

    await select('sources')
    expect(screen.getByText('design.png')).toBeInTheDocument()

    await select('audit')
    expect(screen.getByText('Command failed')).toBeInTheDocument()
  })

  it('keeps historical audit envelopes when live events arrive', async () => {
    render(
      <TaskWorkbench
        client={workbenchClient(vi.fn(), [historicalAuditEvent])}
        events={events}
        projection={projection}
      />,
    )

    await select('audit')
    expect(screen.getByText('engine.run_started')).toBeInTheDocument()
    expect(screen.getByText('permission.requested')).toBeInTheDocument()
  })

  it('stacks narrow layouts and uses the selected desktop width mode', () => {
    const { rerender } = render(
      <TaskWorkbench client={workbenchClient()} events={events} projection={projection} />,
    )

    const workbench = screen.getByRole('complementary', { name: 'Task workbench' })
    expect(workbench).toHaveClass('static', 'w-full', 'task-workbench-panel')
    expect(workbench).toHaveAttribute('data-mode', 'inspector')

    act(() => uiStore.setState({ taskWorkbenchMode: 'collaboration' }))
    rerender(<TaskWorkbench client={workbenchClient()} events={events} projection={projection} />)
    expect(workbench).toHaveAttribute('data-mode', 'collaboration')
  })
})

async function select(
  panel: Parameters<typeof selection>[0],
  overrides: Partial<TaskWorkbenchSelection> = {},
) {
  act(() => uiStore.setState({ taskWorkbenchSelection: selection(panel, overrides) }))
  await waitFor(() =>
    expect(screen.getByRole('tab', { name: panelLabel(panel) })).toHaveAttribute(
      'aria-selected',
      'true',
    ),
  )
}

function selection(
  panel: 'agents' | 'audit' | 'changes' | 'commands' | 'environment' | 'sources',
  overrides: Partial<TaskWorkbenchSelection> = {},
): TaskWorkbenchSelection {
  return {
    eventId,
    panel,
    segmentId,
    taskId,
    ...overrides,
  }
}

function panelLabel(panel: ReturnType<typeof selection>['panel']) {
  return {
    agents: 'Agents',
    audit: 'Audit',
    changes: 'Changes',
    commands: 'Commands',
    environment: 'Environment',
    sources: 'Sources',
  }[panel]
}

function blob(text: string) {
  return {
    blobId,
    bytes: new TextEncoder().encode(text),
    contentHash: Array.from({ length: 32 }, () => 1),
    mediaType: 'text/plain',
    missing: false,
    size: text.length,
  }
}

function workbenchClient(readBlob = vi.fn(), auditEvents: TaskEventEnvelope[] = []) {
  return {
    loadTaskEvents: vi.fn().mockResolvedValue({
      events: auditEvents,
      nextBeforeOffset: null,
      taskId,
    }),
    readBlob,
  }
}

const taskId = '01J00000000000000000000001'
const segmentId = '01J00000000000000000000002'
const eventId = '01J00000000000000000000003'
const blobId = '01J00000000000000000000004'

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
