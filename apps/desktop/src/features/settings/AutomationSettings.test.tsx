import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { AutomationRunRecord, AutomationSpec } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import { DaemonClientProvider } from '@/shared/tauri/react'

import { AutomationSettings } from './AutomationSettings'

function createAutomationDaemonClient(
  options: { automations?: AutomationSpec[]; error?: Error; runs?: AutomationRunRecord[] } = {},
) {
  const reject = options.error ? () => Promise.reject(options.error) : undefined
  return {
    deleteAutomation: vi.fn(async (_workspaceRoot, automationId) => ({
      automationId,
      type: 'automation_deleted' as const,
    })),
    listAutomationRuns:
      reject ?? vi.fn(async () => ({ runs: options.runs ?? [], type: 'automation_runs' as const })),
    listAutomations:
      reject ??
      vi.fn(async () => ({
        automations: options.automations ?? [],
        type: 'automations' as const,
      })),
    runAutomationNow: vi.fn(async (_workspaceRoot, automationId) => ({
      run: {
        automationId,
        id: '01J00000000000000000000001',
        startedAt: '2026-06-30T01:00:00Z',
        status: 'started' as const,
      },
      type: 'automation_run' as const,
    })),
    saveAutomation: vi.fn(async (_workspaceRoot, record) => ({
      automation: record,
      type: 'automation_saved' as const,
    })),
    setAutomationEnabled: vi.fn(async (_workspaceRoot, automationId, enabled) => ({
      automation: automation({ enabled, id: automationId }),
      type: 'automation_enabled' as const,
    })),
  } as unknown as DaemonClient
}

function renderAutomationSettings(daemonClient: DaemonClient = createAutomationDaemonClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={daemonClient}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </DaemonClientProvider>
    )
  }

  return render(<AutomationSettings />, { wrapper: Wrapper })
}

function automation(overrides: Partial<AutomationSpec> = {}): AutomationSpec {
  return {
    createdAt: '2026-06-30T01:00:00Z',
    enabled: false,
    id: 'checks',
    missedRunPolicy: 'skip',
    permissionMode: 'default',
    prompt: 'Run checks',
    sandboxMode: 'none',
    schedule: { intervalMinutes: 30 },
    toolProfile: 'coding',
    updatedAt: '2026-06-30T01:00:00Z',
    workspaceAccess: 'read_only',
    workspaceScope: 'current_workspace',
    ...overrides,
  }
}

describe('AutomationSettings', () => {
  it('renders loading state while automations load', () => {
    renderAutomationSettings({
      ...createAutomationDaemonClient(),
      listAutomations: vi.fn(() => new Promise<never>(() => undefined)),
    } as DaemonClient)

    expect(screen.getByText('正在加载自动化任务。')).toBeInTheDocument()
  })

  it('renders sanitized error state when automations cannot load', async () => {
    renderAutomationSettings(
      createAutomationDaemonClient({
        error: new Error('Authorization=Bearer automation-secret-token'),
      }),
    )

    expect(await screen.findByText('自动化任务无法加载。')).toBeInTheDocument()
    expect(screen.queryByText(/automation-secret-token/)).not.toBeInTheDocument()
  })

  it('renders empty state when no automations exist', async () => {
    renderAutomationSettings(createAutomationDaemonClient())

    expect(await screen.findByText('暂无自动化任务。')).toBeInTheDocument()
  })

  it('shows global write controls when no project is active', async () => {
    renderAutomationSettings(createAutomationDaemonClient())

    expect(await screen.findByLabelText('任务 ID')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '保存自动化任务' })).toBeInTheDocument()
    expect(screen.queryByText('项目覆盖')).not.toBeInTheDocument()
    expect(await screen.findByText('暂无自动化任务。')).toBeInTheDocument()
  })

  it('saves an automation without credentials or raw output', async () => {
    const saveAutomation = vi.fn(async (_workspaceRoot, record) => ({
      automation: record,
      type: 'automation_saved' as const,
    }))
    const client = {
      ...createAutomationDaemonClient(),
      saveAutomation,
    } as DaemonClient

    renderAutomationSettings(client)

    fireEvent.change(await screen.findByLabelText('任务 ID'), { target: { value: 'nightly' } })
    fireEvent.change(screen.getByLabelText('Prompt'), { target: { value: 'Run nightly checks' } })
    fireEvent.change(screen.getByLabelText('间隔分钟'), { target: { value: '45' } })
    fireEvent.change(screen.getByLabelText('工具配置'), { target: { value: 'coding' } })
    fireEvent.change(screen.getByLabelText('权限模式'), { target: { value: 'default' } })
    fireEvent.change(screen.getByLabelText('错过运行策略'), { target: { value: 'run_once' } })
    fireEvent.click(screen.getByRole('button', { name: '保存自动化任务' }))

    await waitFor(() => {
      expect(saveAutomation).toHaveBeenCalledWith(
        undefined,
        expect.objectContaining({
          enabled: false,
          id: 'nightly',
          missedRunPolicy: 'run_once',
          permissionMode: 'default',
          prompt: 'Run nightly checks',
          sandboxMode: 'none',
          schedule: { intervalMinutes: 45 },
          toolProfile: 'coding',
          workspaceAccess: 'read_only',
          workspaceScope: 'current_workspace',
        }),
      )
    })
    expect(JSON.stringify(saveAutomation.mock.calls)).not.toContain('token')
    expect(JSON.stringify(saveAutomation.mock.calls)).not.toContain('raw')
  })

  it('rejects secret-like prompts before saving', async () => {
    const saveAutomation = vi.fn()
    const client = {
      ...createAutomationDaemonClient(),
      saveAutomation,
    } as DaemonClient

    renderAutomationSettings(client)

    fireEvent.change(await screen.findByLabelText('任务 ID'), { target: { value: 'nightly' } })
    fireEvent.change(screen.getByLabelText('Prompt'), {
      target: { value: 'Use token=automation-secret-123456' },
    })
    fireEvent.click(screen.getByRole('button', { name: '保存自动化任务' }))

    expect(await screen.findByText('Prompt 不能包含明文密钥。')).toBeInTheDocument()
    expect(saveAutomation).not.toHaveBeenCalled()
  })

  it('withholds secret-like prompts from saved automation cards', async () => {
    renderAutomationSettings(
      createAutomationDaemonClient({
        automations: [
          automation({
            prompt: 'Use token=automation-secret-123456',
          }),
        ],
      }),
    )

    const card = await screen.findByRole('article', { name: 'checks' })

    expect(within(card).getByText('Prompt 已隐藏。')).toBeInTheDocument()
    expect(within(card).queryByText(/automation-secret/)).not.toBeInTheDocument()
  })

  it('toggles, runs, deletes, and displays recent records', async () => {
    const setAutomationEnabled = vi.fn(async (_workspaceRoot, id, enabled) => ({
      automation: automation({ enabled, id }),
      type: 'automation_enabled' as const,
    }))
    const runAutomationNow = vi.fn(async (_workspaceRoot, id) => ({
      run: {
        automationId: id,
        completedAt: '2026-06-30T01:01:00Z',
        id: 'automation-run-002',
        message: 'Permission/profile snapshot is missing.',
        startedAt: '2026-06-30T01:00:00Z',
        status: 'rejected' as const,
      },
      type: 'automation_run' as const,
    }))
    const deleteAutomation = vi.fn(async (_workspaceRoot, automationId) => ({
      automationId,
      type: 'automation_deleted' as const,
    }))
    const client = {
      ...createAutomationDaemonClient({
        automations: [automation()],
        runs: [
          {
            automationId: 'checks',
            completedAt: '2026-06-30T01:01:00Z',
            id: 'automation-run-001',
            message: 'Permission/profile snapshot is missing.',
            startedAt: '2026-06-30T01:00:00Z',
            status: 'rejected',
          },
        ],
      }),
      deleteAutomation,
      runAutomationNow,
      setAutomationEnabled,
    } as DaemonClient

    renderAutomationSettings(client)

    const card = await screen.findByRole('article', { name: 'checks' })
    expect(within(card).getByText('Run checks')).toBeInTheDocument()
    expect(within(card).getByText('30 分钟')).toBeInTheDocument()
    expect(await screen.findByText('rejected')).toBeInTheDocument()

    fireEvent.click(within(card).getByRole('switch', { name: '启用 checks' }))
    fireEvent.click(within(card).getByRole('button', { name: '立即运行 checks' }))
    fireEvent.click(within(card).getByRole('button', { name: '删除 checks' }))

    await waitFor(() =>
      expect(setAutomationEnabled).toHaveBeenCalledWith(undefined, 'checks', true),
    )
    await waitFor(() => expect(runAutomationNow).toHaveBeenCalledWith(undefined, 'checks'))
    await waitFor(() => expect(deleteAutomation).toHaveBeenCalledWith(undefined, 'checks'))
  })
})
