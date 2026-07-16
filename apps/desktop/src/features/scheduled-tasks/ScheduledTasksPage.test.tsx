import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { ScheduledTaskRunRecord, ScheduledTaskSpec } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { createAppI18n } from '@/shared/i18n/i18n'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider, DaemonClientProvider } from '@/shared/tauri/react'

import { ScheduledTasksPage } from './ScheduledTasksPage'

const navigate = vi.hoisted(() => vi.fn())

vi.mock('@tanstack/react-router', () => ({ useNavigate: () => navigate }))

function scheduledTask(overrides: Partial<ScheduledTaskSpec> = {}): ScheduledTaskSpec {
  return {
    createdAt: '2026-07-16T01:00:00Z',
    enabled: true,
    id: 'task-001',
    missedRunPolicy: 'skip',
    name: 'Project health check',
    permissionMode: 'default',
    prompt: 'Inspect the repository and summarize failing checks.',
    schedule: { intervalMinutes: 30 },
    updatedAt: '2026-07-16T01:00:00Z',
    workspaceRoot: '/repo/alpha',
    ...overrides,
  }
}

function scheduledRun(overrides: Partial<ScheduledTaskRunRecord> = {}): ScheduledTaskRunRecord {
  return {
    id: 'run-001',
    scheduledTaskId: 'task-001',
    startedAt: '2026-07-16T01:30:00Z',
    status: 'succeeded',
    taskId: '01J00000000000000000000001',
    ...overrides,
  }
}

function createDaemonClient(tasks: ScheduledTaskSpec[] = [], runs: ScheduledTaskRunRecord[] = []) {
  return {
    connect: vi.fn().mockResolvedValue({}),
    deleteScheduledTask: vi.fn(async (scheduledTaskId) => ({
      scheduledTaskId,
      type: 'scheduled_task_deleted' as const,
    })),
    listScheduledTaskRuns: vi.fn(async () => ({ runs, type: 'scheduled_task_runs' as const })),
    listScheduledTasks: vi.fn(async () => ({
      scheduledTasks: tasks,
      type: 'scheduled_tasks' as const,
    })),
    runScheduledTaskNow: vi.fn(async (scheduledTaskId) => ({
      run: scheduledRun({ scheduledTaskId, status: 'started' }),
      type: 'scheduled_task_run' as const,
    })),
    saveScheduledTask: vi.fn(async (record) => ({
      scheduledTask: record,
      type: 'scheduled_task_saved' as const,
    })),
    setScheduledTaskEnabled: vi.fn(async (scheduledTaskId, enabled) => ({
      scheduledTask: scheduledTask({ enabled, id: scheduledTaskId }),
      type: 'scheduled_task_enabled' as const,
    })),
  } as unknown as DaemonClient
}

function renderPage(daemonClient: DaemonClient = createDaemonClient()) {
  const commandClient = {
    getDefaultWorkspace: vi.fn(async () => ({ path: '/workspace/default' })),
    listProjects: vi.fn(async () => ({
      activePath: null,
      projects: [{ lastOpenedAt: '2026-07-16T00:00:00Z', name: 'Alpha', path: '/repo/alpha' }],
    })),
  } as unknown as CommandClient
  const queryClient = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <I18nextProvider i18n={createAppI18n('en-US')}>
        <CommandClientProvider client={commandClient}>
          <DaemonClientProvider client={daemonClient}>
            <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
          </DaemonClientProvider>
        </CommandClientProvider>
      </I18nextProvider>
    )
  }

  return render(<ScheduledTasksPage />, { wrapper: Wrapper })
}

describe('ScheduledTasksPage', () => {
  it('renders the standalone empty state', async () => {
    renderPage()

    expect(await screen.findByRole('heading', { name: 'Scheduled tasks' })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'No scheduled tasks' })).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'New scheduled task' })).toHaveLength(2)
  })

  it('shows task state, details, and localized run history', async () => {
    renderPage(createDaemonClient([scheduledTask()], [scheduledRun()]))

    expect((await screen.findAllByText('Project health check')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('Enabled').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Every 30 minutes').length).toBeGreaterThan(0)

    fireEvent.click(screen.getAllByText('Project health check')[0] as HTMLElement)

    expect(await screen.findByText('Recent runs')).toBeInTheDocument()
    expect(screen.getAllByText('Succeeded').length).toBeGreaterThan(0)
    fireEvent.click(screen.getAllByRole('button', { name: 'Open conversation' })[0] as HTMLElement)
    expect(navigate).toHaveBeenCalledWith({
      search: { taskId: '01J00000000000000000000001' },
      to: '/',
    })
  })

  it('creates an enabled task with the selected interval and project', async () => {
    const user = userEvent.setup()
    const client = createDaemonClient()
    renderPage(client)

    await user.click(
      (await screen.findAllByRole('button', { name: 'New scheduled task' }))[0] as HTMLElement,
    )
    await user.type(screen.getByRole('textbox', { name: 'Task name' }), 'Daily summary')
    await user.type(screen.getByRole('textbox', { name: 'Prompt' }), 'Summarize repository status')
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Run interval (minutes)' }), {
      target: { value: '45' },
    })
    fireEvent.change(screen.getByRole('combobox', { name: 'Run in project' }), {
      target: { value: '/repo/alpha' },
    })
    await user.click(screen.getByRole('button', { name: 'Save and enable' }))

    await waitFor(() => expect(client.saveScheduledTask).toHaveBeenCalledOnce())
    expect(client.saveScheduledTask).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        name: 'Daily summary',
        prompt: 'Summarize repository status',
        schedule: { intervalMinutes: 45 },
        workspaceRoot: '/repo/alpha',
      }),
    )
  })

  it('runs, pauses, and deletes a task from row actions', async () => {
    const user = userEvent.setup()
    const client = createDaemonClient([scheduledTask()])
    renderPage(client)

    await screen.findAllByText('Project health check')
    await user.click(
      screen.getAllByRole('button', { name: 'Project health check actions' })[0] as HTMLElement,
    )
    await user.click(screen.getByRole('menuitem', { name: 'Run now' }))
    await waitFor(() => expect(client.runScheduledTaskNow).toHaveBeenCalledWith('task-001'))

    await user.click(
      screen.getAllByRole('button', { name: 'Project health check actions' })[0] as HTMLElement,
    )
    await user.click(screen.getByRole('menuitem', { name: 'Pause' }))
    await waitFor(() =>
      expect(client.setScheduledTaskEnabled).toHaveBeenCalledWith('task-001', false),
    )

    await user.click(
      screen.getAllByRole('button', { name: 'Project health check actions' })[0] as HTMLElement,
    )
    await user.click(screen.getByRole('menuitem', { name: 'Delete' }))
    expect(
      screen.getByRole('heading', { name: 'Delete Project health check?' }),
    ).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Delete task' }))
    await waitFor(() => expect(client.deleteScheduledTask).toHaveBeenCalledWith('task-001'))
  })
})
