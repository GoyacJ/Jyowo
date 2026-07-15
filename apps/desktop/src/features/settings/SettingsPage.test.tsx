import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DaemonClient } from '@/shared/daemon/client'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider, DaemonClientProvider } from '@/shared/tauri/react'
import {
  createRejectedTestCommandClient,
  createTestCommandClient,
  type TestCommandClientOptions,
} from '@/testing/command-client'

import { SettingsPage } from './SettingsPage'

const routerSpy = vi.hoisted(() => ({
  navigate: vi.fn(async ({ search, to }: { search?: Record<string, string>; to: string }) => {
    const nextSearch = search ? `?${new URLSearchParams(search).toString()}` : ''
    window.history.pushState(null, '', `${to}${nextSearch}`)
  }),
}))

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerSpy.navigate,
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { search: Record<string, unknown> } }) => unknown
  }) =>
    select({
      location: {
        search: Object.fromEntries(new URLSearchParams(window.location.search)),
      },
    }),
}))

const emptyProviderSettingsList = {
  defaultConfigId: null,
  selectionScope: 'global' as const,
  configs: [],
}

function renderSettingsPage(options: TestCommandClientOptions = {}, commandClient?: CommandClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    const daemonClient = {
      listAutomationRuns: vi.fn(async () => ({ runs: [], type: 'automation_runs' as const })),
      listAutomations: vi.fn(async () => ({ automations: [], type: 'automations' as const })),
    } as unknown as DaemonClient
    return (
      <CommandClientProvider
        client={
          commandClient ??
          createTestCommandClient({
            providerSettingsList: emptyProviderSettingsList,
            ...options,
          })
        }
      >
        <DaemonClientProvider client={daemonClient}>
          <QueryClientProvider client={queryClient}>
            <AppI18nProvider>{children}</AppI18nProvider>
          </QueryClientProvider>
        </DaemonClientProvider>
      </CommandClientProvider>
    )
  }

  return render(<SettingsPage />, { wrapper: Wrapper })
}

describe('SettingsPage', () => {
  beforeEach(() => {
    routerSpy.navigate.mockClear()
    window.history.pushState(null, '', '/settings')
  })

  afterEach(() => {
    uiStore.getState().setLocale('zh-CN')
    uiStore.getState().setTheme('light')
  })

  it('switches the app language from local settings', () => {
    renderSettingsPage()

    expect(screen.getByRole('region', { name: '设置' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '语言' })).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('应用语言'), { target: { value: 'en-US' } })

    expect(uiStore.getState().locale).toBe('en-US')
    expect(screen.getByRole('region', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Language' })).toBeInTheDocument()
  })

  it('switches the app theme from local settings', () => {
    renderSettingsPage()

    expect(screen.getByRole('heading', { name: '主题' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '浅色' })).toHaveAttribute('aria-pressed', 'true')

    fireEvent.click(screen.getByRole('button', { name: '深色' }))

    expect(uiStore.getState().theme).toBe('dark')
    expect(screen.getByRole('button', { name: '深色' })).toHaveAttribute('aria-pressed', 'true')

    fireEvent.click(screen.getByRole('button', { name: '跟随系统' }))

    expect(uiStore.getState().theme).toBe('system')
    expect(screen.getByRole('button', { name: '跟随系统' })).toHaveAttribute('aria-pressed', 'true')
  })

  it('renders settings sections as top-level tabs', async () => {
    renderSettingsPage()

    expect(screen.getByRole('tab', { name: '通用' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('tab', { name: '技能' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '工具' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '自动化' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'MCP' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '插件' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '模型' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '关于' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '语言' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '模型配置' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '技能' }))

    expect(
      await screen.findByRole('button', { name: /Creates release notes from recent changes/ }),
    ).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(await screen.findByRole('heading', { name: '工具' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '自动化' }))

    expect(await screen.findByRole('heading', { name: '自动化任务' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'MCP' }))

    expect(await screen.findByRole('heading', { name: 'MCP 服务器' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '插件' }))

    expect(await screen.findByRole('heading', { name: '插件' })).toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '模型' }))

    expect(await screen.findByRole('heading', { name: '模型' })).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: '暂无已配置模型' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '模型配置' })).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '关于' }))

    expect(await screen.findByRole('heading', { name: '关于 Jyowo' })).toBeInTheDocument()
  })

  it('opens the requested settings tab from route search', async () => {
    window.history.pushState(null, '', '/settings?tab=plugins')

    renderSettingsPage()

    expect(screen.getByRole('tab', { name: '插件' })).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByRole('heading', { name: '插件' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '语言' })).not.toBeInTheDocument()
  })

  it('writes selected settings tab to route search', async () => {
    renderSettingsPage()

    fireEvent.mouseDown(screen.getByRole('tab', { name: '自动化' }))

    expect(await screen.findByRole('heading', { name: '自动化任务' })).toBeInTheDocument()
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { tab: 'automations' },
      to: '/settings',
    })
  })

  it('renders backend-authored runtime execution status in the tools tab', async () => {
    renderSettingsPage({
      runtimeTools: {
        generation: 4,
        scope: 'project',
        customized: false,
        tools: [
          {
            name: 'GitStatus',
            displayName: 'Git status',
            description: 'Show repository status.',
            category: 'builtin',
            group: 'git',
            groupLabel: 'Git',
            originKind: 'builtin',
            originId: null,
            access: 'readOnly',
            executionChannel: 'directAuthorizedRust',
            requiredCapabilities: [],
            deferPolicy: 'alwaysLoad',
            longRunning: false,
            serviceBinding: null,
            configuredEnabled: true,
            available: true,
            unavailableReason: null,
          },
        ],
      },
      runtimeExecutionStatus: {
        processSandbox: {
          backendId: 'routing',
          candidateIds: ['local-process', 'docker-process'],
          availableNetworkPolicies: ['none', 'allowlist'],
          availableWorkspacePolicies: ['read_only', 'writable_subpaths'],
          unavailableReasons: [],
        },
        httpBroker: {
          available: false,
          deniedReasons: ['network broker is not registered in the capability registry'],
        },
        tools: [
          {
            toolName: 'Bash',
            available: true,
            unavailableReason: null,
          },
          {
            toolName: 'WebFetch',
            available: false,
            unavailableReason: 'HTTP broker is not registered',
          },
        ],
      },
    })

    fireEvent.mouseDown(screen.getByRole('tab', { name: '工具' }))

    expect(await screen.findByRole('heading', { name: '工具' })).toBeInTheDocument()
    expect(await screen.findByText('GitStatus')).toBeInTheDocument()
    expect(await screen.findByText('沙箱 routing')).toBeInTheDocument()
    fireEvent.click(screen.getByText('查看详情'))
    expect(screen.getByText('local-process, docker-process')).toBeInTheDocument()
    expect(
      screen.getByText('network broker is not registered in the capability registry'),
    ).toBeInTheDocument()
    expect(screen.queryByText('WebFetch')).not.toBeInTheDocument()
  })

  it('opens runtime tools from the tools route search', async () => {
    window.history.replaceState(null, '', '/settings?tab=tools')

    renderSettingsPage({
      runtimeTools: {
        generation: 5,
        scope: 'project',
        customized: false,
        tools: [
          {
            name: 'GitStatus',
            displayName: 'Git status',
            description: 'Show repository status.',
            category: 'builtin',
            group: 'git',
            groupLabel: 'Git',
            originKind: 'builtin',
            originId: null,
            access: 'readOnly',
            executionChannel: 'directAuthorizedRust',
            requiredCapabilities: [],
            deferPolicy: 'alwaysLoad',
            longRunning: false,
            serviceBinding: null,
            configuredEnabled: true,
            available: true,
            unavailableReason: null,
          },
        ],
      },
    })

    expect(screen.getByRole('tab', { name: '工具' })).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByRole('heading', { name: '工具' })).toBeInTheDocument()
    expect(await screen.findByText('GitStatus')).toBeInTheDocument()
  })

  it('renders backend errors in the tools tab', async () => {
    window.history.replaceState(null, '', '/settings?tab=tools')

    renderSettingsPage(
      {},
      createRejectedTestCommandClient('desktop settings runtime is not initialized'),
    )

    await waitFor(() => {
      expect(screen.getAllByText('desktop settings runtime is not initialized')).toHaveLength(2)
    })
  })

  it('owns the right pane scroll container', () => {
    renderSettingsPage()

    expect(screen.getByRole('region', { name: '设置' })).toHaveClass('h-full')
    expect(screen.getByRole('region', { name: '设置' })).toHaveClass('overflow-y-auto')
  })
})
