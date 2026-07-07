import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { AppI18nProvider } from '@/shared/i18n/i18n'
import type { AutomationSpec, CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createRejectedTestCommandClient, createTestCommandClient } from '@/testing/command-client'

import { AutomationSettings } from './AutomationSettings'

function renderAutomationSettings(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </CommandClientProvider>
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
      ...createTestCommandClient(),
      listAutomations: vi.fn(() => new Promise<never>(() => undefined)),
    })

    expect(screen.getByText('正在加载自动化任务。')).toBeInTheDocument()
  })

  it('renders sanitized error state when automations cannot load', async () => {
    renderAutomationSettings(
      createRejectedTestCommandClient(new Error('Authorization=Bearer automation-secret-token')),
    )

    expect(await screen.findByText('自动化任务无法加载。')).toBeInTheDocument()
    expect(screen.queryByText(/automation-secret-token/)).not.toBeInTheDocument()
  })

  it('renders empty state when no automations exist', async () => {
    renderAutomationSettings(
      createTestCommandClient({
        automationRuns: { runs: [] },
        automations: { automations: [] },
      }),
    )

    expect(await screen.findByText('暂无自动化任务。')).toBeInTheDocument()
  })

  it('renders runtime diagnostics without project write controls when no project is active', async () => {
    renderAutomationSettings(
      createTestCommandClient({
        automationRuns: { runs: [] },
        automations: { automations: [] },
        projects: { activePath: null, projects: [] },
      }),
    )

    expect(await screen.findByText('运行时诊断')).toBeInTheDocument()
    expect(screen.queryByLabelText('任务 ID')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '保存自动化任务' })).not.toBeInTheDocument()
    expect(await screen.findByText('暂无自动化任务。')).toBeInTheDocument()
  })

  it('saves an automation without credentials or raw output', async () => {
    const saveAutomation = vi.fn(async (request) => ({
      automation: request.automation,
      status: 'saved' as const,
    }))
    const client = {
      ...createTestCommandClient({
        automationRuns: { runs: [] },
        automations: { automations: [] },
      }),
      saveAutomation,
    } satisfies CommandClient

    renderAutomationSettings(client)

    fireEvent.change(await screen.findByLabelText('任务 ID'), { target: { value: 'nightly' } })
    fireEvent.change(screen.getByLabelText('Prompt'), { target: { value: 'Run nightly checks' } })
    fireEvent.change(screen.getByLabelText('间隔分钟'), { target: { value: '45' } })
    fireEvent.change(screen.getByLabelText('工具配置'), { target: { value: 'coding' } })
    fireEvent.change(screen.getByLabelText('权限模式'), { target: { value: 'default' } })
    fireEvent.change(screen.getByLabelText('错过运行策略'), { target: { value: 'run_once' } })
    fireEvent.click(screen.getByRole('button', { name: '保存自动化任务' }))

    await waitFor(() => {
      expect(saveAutomation).toHaveBeenCalledWith({
        automation: expect.objectContaining({
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
      })
    })
    expect(JSON.stringify(saveAutomation.mock.calls)).not.toContain('token')
    expect(JSON.stringify(saveAutomation.mock.calls)).not.toContain('raw')
  })

  it('rejects secret-like prompts before saving', async () => {
    const saveAutomation = vi.fn()
    const client = {
      ...createTestCommandClient({
        automationRuns: { runs: [] },
        automations: { automations: [] },
      }),
      saveAutomation,
    } satisfies CommandClient

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
      createTestCommandClient({
        automationRuns: { runs: [] },
        automations: {
          automations: [
            automation({
              prompt: 'Use token=automation-secret-123456',
            }),
          ],
        },
      }),
    )

    const card = await screen.findByRole('article', { name: 'checks' })

    expect(within(card).getByText('Prompt 已隐藏。')).toBeInTheDocument()
    expect(within(card).queryByText(/automation-secret/)).not.toBeInTheDocument()
  })

  it('toggles, runs, deletes, and displays recent records', async () => {
    const setAutomationEnabled = vi.fn(async (id, enabled) => ({
      automation: automation({ enabled, id }),
      status: 'saved' as const,
    }))
    const runAutomationNow = vi.fn(async (id) => ({
      record: {
        automationId: id,
        completedAt: '2026-06-30T01:01:00Z',
        id: 'automation-run-002',
        message: 'Permission/profile snapshot is missing.',
        startedAt: '2026-06-30T01:00:00Z',
        status: 'rejected' as const,
      },
    }))
    const deleteAutomation = vi.fn(async (id) => ({ id, status: 'deleted' as const }))
    const client = {
      ...createTestCommandClient({
        automationRuns: {
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
        },
        automations: { automations: [automation()] },
      }),
      deleteAutomation,
      runAutomationNow,
      setAutomationEnabled,
    } satisfies CommandClient

    renderAutomationSettings(client)

    const card = await screen.findByRole('article', { name: 'checks' })
    expect(within(card).getByText('Run checks')).toBeInTheDocument()
    expect(within(card).getByText('30 分钟')).toBeInTheDocument()
    expect(await screen.findByText('rejected')).toBeInTheDocument()

    fireEvent.click(within(card).getByRole('switch', { name: '启用 checks' }))
    fireEvent.click(within(card).getByRole('button', { name: '立即运行 checks' }))
    fireEvent.click(within(card).getByRole('button', { name: '删除 checks' }))

    await waitFor(() => expect(setAutomationEnabled).toHaveBeenCalledWith('checks', true))
    await waitFor(() => expect(runAutomationNow).toHaveBeenCalledWith('checks'))
    await waitFor(() => expect(deleteAutomation).toHaveBeenCalledWith('checks'))
  })
})
