import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'

import { AppShell } from './AppShell'

const routerMock = vi.hoisted(() => ({
  navigate: vi.fn(async ({ to }: { to: string }) => {
    window.history.pushState(null, '', to)
  }),
}))

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerMock.navigate,
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

function renderAppShell(commandClient: CommandClient = createMockCommandClient()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
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

function mockCompactViewport(matches: boolean) {
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
    mockCompactViewport(false)
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().setContextPanelCollapsed(true)
      uiStore.getState().clearActiveRun()
      uiStore.getState().clearTimelineScrollRequest()
    })
  })

  it('provides the conversation-first desktop workspace regions', async () => {
    const { container } = renderAppShell()

    expect(container.firstElementChild).toHaveClass('h-screen')
    expect(container.firstElementChild).toHaveClass('overflow-hidden')
    expect(screen.getByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Workspace' })).toBeInTheDocument()
    expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
    expect(screen.getByRole('region', { name: 'Status' })).toBeInTheDocument()

    const workspaceNavigation = screen.getByRole('navigation', { name: 'Workspace' })
    expect(workspaceNavigation).not.toHaveClass('hidden')
    expect(within(workspaceNavigation).getByText('Recent conversations')).toBeInTheDocument()
    expect(
      within(workspaceNavigation).getByRole('button', { name: 'Switch project' }),
    ).toBeInTheDocument()
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
    expect(screen.getByRole('button', { name: 'More actions' })).toBeDisabled()
    expect(screen.queryByRole('button', { name: 'Share' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Hide context panel' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show context panel' })).toBeInTheDocument()
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

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/settings' })
  })

  it('uses the icon sidebar on narrow viewports', () => {
    mockCompactViewport(true)

    renderAppShell()

    const workspaceNavigation = screen.getByRole('navigation', { name: 'Workspace' })

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

    fireEvent.click(screen.getByRole('option', { name: 'Open evals' }))

    await waitFor(() => {
      expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/evals' })
    })
  })

  it('does not request hidden context while idle', async () => {
    const commandClient = createMockCommandClient()
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    renderAppShell(trackedClient)

    await waitFor(() => {
      expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
      expect(contextRequests).toEqual([])
    })
  })

  it('keeps active conversation context collapsed until the user opens it', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-002')
    const commandClient = createMockCommandClient({
      conversations: {
        conversations: [
          {
            id: 'conversation-001',
            isEmpty: false,
            title: 'First conversation',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
          {
            id: 'conversation-002',
            isEmpty: false,
            title: 'Selected conversation',
            updatedAt: '2026-06-17T00:00:01.000Z',
          },
        ],
      },
    })
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-002',
        runId: 'run-002',
      })
    })

    renderAppShell(trackedClient)

    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(contextRequests).toEqual([])

    fireEvent.click(screen.getByRole('button', { name: 'Show context panel' }))

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    await waitFor(() => {
      expect(contextRequests).toEqual([{ conversationId: 'conversation-002', runId: 'run-002' }])
    })
  })

  it('opens selected idle conversation context as a fixed right column', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-002')
    const commandClient = createMockCommandClient()
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    const { container } = renderAppShell(trackedClient)

    fireEvent.click(screen.getByRole('button', { name: 'Show context panel' }))

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    expect(container.firstElementChild?.firstElementChild).toHaveStyle({
      gridTemplateColumns: '248px minmax(0,1fr) 320px',
    })
    expect(contextRequests).toEqual([{ conversationId: 'conversation-002' }])
  })

  it('uses the selected conversation run when multiple conversations are active', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-001')
    const commandClient = createMockCommandClient()
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-002',
        runId: 'run-002',
      })
    })

    renderAppShell(trackedClient)

    fireEvent.click(screen.getByRole('button', { name: 'Show context panel' }))

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    await waitFor(() => {
      expect(contextRequests).toEqual([{ conversationId: 'conversation-001', runId: 'run-001' }])
    })
  })

  it('does not show another conversation run while the selected conversation is idle', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-001')
    const commandClient = createMockCommandClient()
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-002',
        runId: 'run-002',
      })
    })

    renderAppShell(trackedClient)

    await waitFor(() => {
      expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
      expect(contextRequests).toEqual([])
    })
    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(
      within(screen.getByRole('region', { name: 'Status' })).getByText('Ready'),
    ).toBeInTheDocument()
    expect(
      within(screen.getByRole('region', { name: 'Status' })).queryByText('run-002'),
    ).not.toBeInTheDocument()
  })

  it('keeps active run context off standalone pages', async () => {
    window.history.pushState(null, '', '/settings')
    const commandClient = createMockCommandClient()
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell(trackedClient)

    await waitFor(() => {
      expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
      expect(contextRequests).toEqual([])
    })
    expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Show context panel' })).not.toBeInTheDocument()
  })

  it('minimizes and restores active conversation context', async () => {
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell()

    fireEvent.click(screen.getByRole('button', { name: 'Show context panel' }))

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    fireEvent.click(screen.getByRole('button', { name: 'Close context' }))

    await waitFor(() => {
      expect(screen.queryByRole('complementary', { name: 'Context' })).not.toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole('button', { name: 'Show context panel' }))

    expect(screen.getByRole('complementary', { name: 'Context' })).toBeInTheDocument()
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

  it('does not render shell permission controls; context decisions scroll the timeline', async () => {
    const commandClient = createMockCommandClient({
      contextSnapshot: {
        activeArtifact: 'App shell (WIP)',
        decisions: [
          {
            detail: 'Critical permission is waiting for decision 01HZ0000000000000000000001.',
            requestId: '01HZ0000000000000000000001',
            title: 'Approve shell',
          },
        ],
        files: [{ label: 'src/' }],
        nextActions: ['Resolve pending runtime decisions'],
        path: '~/projects/desktop-app',
        project: 'Desktop App',
      },
    })

    act(() => {
      uiStore.getState().setContextPanelCollapsed(false)
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell(commandClient)

    expect(screen.queryByRole('button', { name: 'Deny permission' })).not.toBeInTheDocument()

    fireEvent.click(await screen.findByRole('button', { name: /Approve shell/i }))

    await waitFor(() => {
      expect(uiStore.getState().timelineScrollRequest).toEqual({
        anchorId: 'permission:01HZ0000000000000000000001',
        nonce: 1,
      })
    })
  })
})
