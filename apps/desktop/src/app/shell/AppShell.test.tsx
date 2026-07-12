import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { taskStoreFor } from '@/features/tasks/use-task'
import type { DaemonClient } from '@/shared/daemon/client'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider, DaemonClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'

import { AppShell } from './AppShell'

const routerSpy = vi.hoisted(() => ({
  navigate: vi.fn(async ({ to }: { to: string }) => {
    window.history.pushState(null, '', to)
  }),
}))

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerSpy.navigate,
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { pathname: string; search: Record<string, unknown> } }) => unknown
  }) =>
    select({
      location: {
        pathname: window.location.pathname,
        search: Object.fromEntries(new URLSearchParams(window.location.search)),
      },
    }),
}))

function renderAppShell(commandClient: CommandClient = createTestCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })
  const daemonClient = {
    connect: vi.fn().mockResolvedValue(undefined),
    listTasks: vi.fn().mockResolvedValue({ tasks: [], type: 'task_list' }),
    subscribe: vi.fn().mockResolvedValue(async () => undefined),
  } as unknown as DaemonClient

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <DaemonClientProvider client={daemonClient}>
          <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
        </DaemonClientProvider>
      </CommandClientProvider>
    )
  }

  return render(
    <AppShell>
      <h1>Workbench content</h1>
    </AppShell>,
    { wrapper: Wrapper },
  )
}

function setCompactViewportFixture(matches: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === '(max-width: 720px)' ? matches : false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

describe('AppShell', () => {
  beforeEach(() => {
    setCompactViewportFixture(false)
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().setContextPanelCollapsed(true)
      uiStore.getState().setInspectorOpen(false)
      uiStore.getState().clearActiveRun()
      uiStore.getState().clearTimelineScrollRequest()
    })
  })

  it('provides the conversation-first desktop workspace regions', async () => {
    const { container } = renderAppShell()

    expect(container.firstElementChild).toHaveClass('h-screen')
    expect(container.firstElementChild).toHaveClass('overflow-hidden')
    expect(container.firstElementChild?.firstElementChild).toHaveStyle({
      gridTemplateColumns: '300px minmax(0,1fr)',
    })
    expect(screen.getByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Workspace' })).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Conversations' })).toBeInTheDocument()
    expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
    expect(screen.getByRole('region', { name: 'Status' })).toBeInTheDocument()

    const workspaceNavigation = screen.getByRole('complementary', { name: 'Workspace' })
    expect(workspaceNavigation).not.toHaveClass('hidden')
    expect(
      within(workspaceNavigation).getByRole('button', { name: 'New conversation' }),
    ).toBeInTheDocument()
    expect(
      within(workspaceNavigation).queryByRole('button', { name: 'Search' }),
    ).not.toBeInTheDocument()
    expect(
      within(workspaceNavigation).queryByRole('button', { name: 'Scheduled' }),
    ).not.toBeInTheDocument()
    expect(
      within(workspaceNavigation).queryByRole('button', { name: 'Plugins' }),
    ).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Home')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Artifacts')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Skills')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Agents')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Tools')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Settings')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Runs')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('MCP')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Memory')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Evals')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Models')).not.toBeInTheDocument()
    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'View all activity' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open inspector' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Share' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Hide context panel' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Show context panel' })).not.toBeInTheDocument()
    const statusBar = screen.getByRole('region', { name: 'Status' })

    expect(within(statusBar).getByText('Ready')).toBeInTheDocument()
    expect(within(statusBar).queryByText('Local')).not.toBeInTheDocument()
    expect(within(statusBar).getByRole('button', { name: 'Settings' })).toBeInTheDocument()
  })

  it('routes settings from the bottom status bar', () => {
    renderAppShell()

    fireEvent.click(
      within(screen.getByRole('region', { name: 'Status' })).getByRole('button', {
        name: 'Settings',
      }),
    )

    expect(routerSpy.navigate).toHaveBeenCalledWith({ to: '/settings' })
    expect(uiStore.getState().inspectorOpen).toBe(false)
    expect(screen.queryByRole('complementary', { name: 'Inspector' })).not.toBeInTheDocument()
  })

  it('uses the icon sidebar on narrow viewports', () => {
    setCompactViewportFixture(true)

    renderAppShell()

    const workspaceNavigation = screen.getByRole('complementary', { name: 'Workspace' })

    expect(workspaceNavigation).toHaveAttribute('data-collapsed', 'true')
    expect(within(workspaceNavigation).queryByText('Recent conversations')).not.toBeInTheDocument()
    expect(
      within(workspaceNavigation).queryByRole('button', { name: 'Skills' }),
    ).not.toBeInTheDocument()
  })

  it('opens the command palette from the top actions and routes commands', async () => {
    renderAppShell()

    fireEvent.click(screen.getByRole('button', { name: 'Open command palette' }))

    expect(screen.getByRole('dialog', { name: 'Command palette' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('option', { name: 'Settings' }))

    await waitFor(() => {
      expect(routerSpy.navigate).toHaveBeenCalledWith({ to: '/settings' })
    })
  })



  it('reflects the selected task generated current run in the status bar', () => {
    const taskId = '01J00000000000000000000041'
    window.history.pushState(null, '', `/?taskId=${taskId}`)
    act(() => {
      taskStoreFor(taskId)
        .getState()
        .replaceSnapshot({
          projection: {
            archived: false,
            currentRun: {
              incompleteOutput: false,
              segmentId: '01J00000000000000000000042',
              startedAt: '2026-07-11T06:00:00Z',
              state: 'running',
            },
            lastGlobalOffset: 4,
            queue: [],
            state: 'running',
            streamVersion: 4,
            taskId,
            title: 'Generated task projection',
          },
          snapshotOffset: 4,
          timeline: [],
        })
    })

    renderAppShell()

    expect(
      within(screen.getByRole('region', { name: 'Status' })).getByText('In progress'),
    ).toBeInTheDocument()
  })


  it('reflects active run state in the status bar from ui store', async () => {
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell()

    const statusBar = screen.getByRole('region', { name: 'Status' })
    expect(within(statusBar).getByText('In progress')).toBeInTheDocument()
    expect(within(statusBar).queryByText('run-001')).not.toBeInTheDocument()
  })

  it('does not render shell permission or right panel controls', () => {
    act(() => {
      uiStore.getState().setContextPanelCollapsed(false)
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell()

    expect(screen.queryByRole('button', { name: 'Deny permission' })).not.toBeInTheDocument()
    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(screen.queryByRole('complementary', { name: 'Inspector' })).not.toBeInTheDocument()
  })
})
