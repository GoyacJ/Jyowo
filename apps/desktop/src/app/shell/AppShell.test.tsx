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

describe('AppShell', () => {
  beforeEach(() => {
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setActivityRailCollapsed(false)
      uiStore.getState().setActivityRailExpanded(false)
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().clearActiveRun()
    })
  })

  it('provides the conversation-first desktop workspace regions', async () => {
    const { container } = renderAppShell()

    expect(container.firstElementChild).toHaveClass('h-screen')
    expect(container.firstElementChild).toHaveClass('overflow-hidden')
    expect(screen.getByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Workspace' })).toBeInTheDocument()
    expect(screen.getByRole('main')).toContainElement(screen.getByText('Workbench content'))
    expect(screen.getByRole('complementary', { name: 'Context' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Activity' })).toBeInTheDocument()

    const workspaceNavigation = screen.getByRole('navigation', { name: 'Workspace' })
    expect(workspaceNavigation).not.toHaveClass('hidden')
    expect(within(workspaceNavigation).getByText('Recent conversations')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Home')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Conversations')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Projects')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Artifacts')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Agents')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Tools')).toBeInTheDocument()
    expect(within(workspaceNavigation).getByText('Settings')).toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Runs')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('MCP')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Memory')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Evals')).not.toBeInTheDocument()
    expect(within(workspaceNavigation).queryByText('Models')).not.toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Context' })).not.toHaveClass('hidden')
    expect(screen.getByRole('button', { name: 'View all activity' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'More actions' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Share' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Hide context panel' })).toBeEnabled()
    const contextPanel = screen.getByRole('complementary', { name: 'Context' })
    const activityRail = screen.getByRole('region', { name: 'Activity' })

    expect(await within(contextPanel).findByText('Desktop App')).toBeInTheDocument()
    expect(within(contextPanel).getByText('src/')).toBeInTheDocument()
    expect(within(activityRail).getByText('Current run')).toBeInTheDocument()
    expect(within(activityRail).getByText('run')).toBeInTheDocument()
  })

  it('requests context and activity for the selected conversation', async () => {
    const commandClient = createMockCommandClient()
    const activityRequests: Array<Parameters<CommandClient['listActivity']>[0]> = []
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
      listActivity: async (request: Parameters<CommandClient['listActivity']>[0]) => {
        activityRequests.push(request)
        return commandClient.listActivity(request)
      },
    } satisfies CommandClient

    renderAppShell(trackedClient)

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    await waitFor(() => {
      expect(contextRequests).toEqual([{}, { conversationId: 'conversation-001' }])
      expect(activityRequests).toEqual([{ conversationId: 'conversation-001' }])
    })
  })

  it('requests context and activity for the conversation selected in the URL', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-002')
    const commandClient = createMockCommandClient({
      conversations: {
        conversations: [
          {
            id: 'conversation-001',
            title: 'First conversation',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
          {
            id: 'conversation-002',
            title: 'Selected conversation',
            updatedAt: '2026-06-17T00:00:01.000Z',
          },
        ],
      },
    })
    const activityRequests: Array<Parameters<CommandClient['listActivity']>[0]> = []
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
      listActivity: async (request: Parameters<CommandClient['listActivity']>[0]) => {
        activityRequests.push(request)
        return commandClient.listActivity(request)
      },
    } satisfies CommandClient

    renderAppShell(trackedClient)

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    await waitFor(() => {
      expect(contextRequests).toEqual([{}, { conversationId: 'conversation-002' }])
      expect(activityRequests).toEqual([{ conversationId: 'conversation-002' }])
    })
  })

  it('requests activity for the active run after a run starts', async () => {
    const commandClient = createMockCommandClient()
    const activityRequests: Array<Parameters<CommandClient['listActivity']>[0]> = []
    const contextRequests: Array<Parameters<CommandClient['getContextSnapshot']>[0]> = []
    const trackedClient = {
      ...commandClient,
      getContextSnapshot: async (request: Parameters<CommandClient['getContextSnapshot']>[0]) => {
        contextRequests.push(request)
        return commandClient.getContextSnapshot(request)
      },
      listActivity: async (request: Parameters<CommandClient['listActivity']>[0]) => {
        activityRequests.push(request)
        return commandClient.listActivity(request)
      },
    } satisfies CommandClient

    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })

    renderAppShell(trackedClient)

    await within(screen.getByRole('complementary', { name: 'Context' })).findByText('Desktop App')

    await waitFor(() => {
      expect(contextRequests).toEqual([{}, { conversationId: 'conversation-001' }])
      expect(activityRequests).toEqual([{ conversationId: 'conversation-001', runId: 'run-001' }])
    })
  })

  it('supports local activity rail collapse and view-all state', async () => {
    renderAppShell()

    expect(
      await within(screen.getByRole('region', { name: 'Activity' })).findByText('run'),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))
    expect(screen.getByRole('region', { name: 'Activity' })).toHaveAttribute(
      'data-expanded',
      'true',
    )
    expect(screen.getByRole('region', { name: 'Replay timeline' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Usage summary' })).toBeInTheDocument()
    expect(screen.getByText('Usage analytics unavailable.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Export support bundle' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Collapse activity' }))
    expect(screen.getByRole('region', { name: 'Activity' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.queryByText('run')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Expand activity' }))
    expect(screen.getByRole('region', { name: 'Activity' })).toHaveAttribute(
      'data-collapsed',
      'false',
    )
  })

  it('renders usage summary from completed run activity events', async () => {
    renderAppShell(
      createMockCommandClient({
        listActivity: {
          events: [
            {
              id: 'evt-started',
              payload: { sessionId: 'conversation-001' },
              runId: 'run-001',
              sequence: 1,
              source: 'engine',
              timestamp: '2026-06-17T00:00:00.000Z',
              type: 'run.started',
              visibility: 'public',
            },
            {
              id: 'evt-ended',
              payload: {
                reason: 'completed',
                usage: {
                  cacheReadTokens: 3,
                  cacheWriteTokens: 5,
                  costMicros: 260,
                  inputTokens: 11,
                  outputTokens: 7,
                  toolCalls: 2,
                },
              },
              runId: 'run-001',
              sequence: 2,
              source: 'engine',
              timestamp: '2026-06-17T00:00:01.000Z',
              type: 'run.ended',
              visibility: 'public',
            },
          ],
        },
      }),
    )

    await waitFor(() => {
      expect(
        within(screen.getByRole('region', { name: 'Activity' })).getAllByText('run'),
      ).toHaveLength(2)
    })

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))

    const usageSummary = screen.getByRole('region', { name: 'Usage summary' })
    expect(within(usageSummary).getByText('11')).toBeInTheDocument()
    expect(within(usageSummary).getByText('7')).toBeInTheDocument()
    expect(within(usageSummary).getByText('2')).toBeInTheDocument()
    expect(within(usageSummary).getByText('$0.000260')).toBeInTheDocument()
  })

  it('exports a redacted support bundle from the expanded activity rail', async () => {
    const commandClient = createMockCommandClient()
    const exportSupportBundle = vi.fn(commandClient.exportSupportBundle)
    const trackedClient = {
      ...commandClient,
      exportSupportBundle,
    } satisfies CommandClient

    renderAppShell(trackedClient)

    expect(
      await within(screen.getByRole('region', { name: 'Activity' })).findByText('run'),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))
    fireEvent.click(screen.getByRole('button', { name: 'Export support bundle' }))

    await waitFor(() => {
      expect(exportSupportBundle).toHaveBeenCalledWith({ conversationId: 'conversation-001' })
    })
    expect(await screen.findByText('Redacted')).toBeInTheDocument()
  })

  it('shows permission details and sends approve or deny intent through the command client', async () => {
    const commandClient = createMockCommandClient({
      listActivity: {
        events: [
          {
            id: 'evt-permission',
            payload: {
              command: {
                argv: ['pnpm', 'install'],
                cwd: 'workspace://local',
                executable: 'pnpm',
              },
              decisionScope: 'current run',
              exposure: 'Can modify package metadata and lockfile.',
              operation: 'Install dependencies',
              reason: 'The run requested package installation.',
              requestId: '01HZ0000000000000000000001',
              severity: 'high',
              target: 'workspace package manager',
              workspaceBoundary: 'workspace://local',
            },
            runId: 'run-001',
            sequence: 1,
            source: 'policy',
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'permission.requested',
            visibility: 'public',
          },
        ],
      },
    })
    const resolvePermission = vi.fn(commandClient.resolvePermission)
    const trackedClient = {
      ...commandClient,
      resolvePermission,
    } satisfies CommandClient

    renderAppShell(trackedClient)

    expect(
      await within(screen.getByRole('region', { name: 'Activity' })).findByText(
        '01HZ0000000000000000000001',
      ),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))

    const details = await screen.findByRole('region', { name: 'Run event details' })
    expect(within(details).getAllByText('Install dependencies')).toHaveLength(2)
    expect(within(details).getByText('workspace package manager')).toBeInTheDocument()
    expect(within(details).getByText('current run')).toBeInTheDocument()

    fireEvent.click(within(details).getByRole('button', { name: 'Approve permission' }))

    await waitFor(() => {
      expect(resolvePermission).toHaveBeenCalledWith({
        decision: 'approve',
        requestId: '01HZ0000000000000000000001',
      })
    })

    fireEvent.click(within(details).getByRole('button', { name: 'Deny permission' }))

    await waitFor(() => {
      expect(resolvePermission).toHaveBeenCalledWith({
        decision: 'deny',
        requestId: '01HZ0000000000000000000001',
      })
    })
  })

  it('sends only one permission decision while a request is resolving', async () => {
    let releaseResolution: (() => void) | undefined
    const commandClient = createMockCommandClient({
      listActivity: {
        events: [
          {
            id: 'evt-permission',
            payload: {
              decisionScope: 'current run',
              exposure: 'Can modify package metadata and lockfile.',
              operation: 'Install dependencies',
              reason: 'The run requested package installation.',
              requestId: '01HZ0000000000000000000001',
              severity: 'high',
              target: 'workspace package manager',
              workspaceBoundary: 'workspace://local',
            },
            runId: 'run-001',
            sequence: 1,
            source: 'policy',
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'permission.requested',
            visibility: 'public',
          },
        ],
      },
    })
    const resolvePermission = vi.fn(
      () =>
        new Promise<Awaited<ReturnType<CommandClient['resolvePermission']>>>((resolve) => {
          releaseResolution = () =>
            resolve({
              decision: 'approve',
              requestId: '01HZ0000000000000000000001',
              status: 'resolved',
            })
        }),
    )
    const trackedClient = {
      ...commandClient,
      resolvePermission,
    } satisfies CommandClient

    renderAppShell(trackedClient)

    expect(
      await within(screen.getByRole('region', { name: 'Activity' })).findByText(
        '01HZ0000000000000000000001',
      ),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))
    const details = await screen.findByRole('region', { name: 'Run event details' })

    fireEvent.click(within(details).getByRole('button', { name: 'Approve permission' }))
    fireEvent.click(within(details).getByRole('button', { name: 'Deny permission' }))

    await waitFor(() => {
      expect(resolvePermission).toHaveBeenCalledTimes(1)
    })
    expect(resolvePermission).toHaveBeenCalledWith({
      decision: 'approve',
      requestId: '01HZ0000000000000000000001',
    })

    releaseResolution?.()
  })

  it('prefers pending permission details over already resolved permissions', async () => {
    const commandClient = createMockCommandClient({
      listActivity: {
        events: [
          {
            id: 'evt-permission-approved',
            payload: {
              decisionScope: 'current run',
              exposure: 'Read file metadata.',
              operation: 'Read files',
              reason: 'The run requested project context.',
              requestId: '01HZ0000000000000000000001',
              severity: 'low',
              target: 'apps/desktop/src',
              workspaceBoundary: 'workspace://local',
            },
            runId: 'run-001',
            sequence: 1,
            source: 'policy',
            timestamp: '2026-06-17T00:00:00.000Z',
            type: 'permission.requested',
            visibility: 'public',
          },
          {
            id: 'evt-permission-approved-resolution',
            payload: {
              decision: 'approve',
              requestId: '01HZ0000000000000000000001',
            },
            runId: 'run-001',
            sequence: 2,
            source: 'policy',
            timestamp: '2026-06-17T00:00:01.000Z',
            type: 'permission.resolved',
            visibility: 'public',
          },
          {
            id: 'evt-permission-pending',
            payload: {
              decisionScope: 'current run',
              exposure: 'Can modify implementation files.',
              operation: 'Write files',
              reason: 'The run needs to apply code changes.',
              requestId: '01HZ0000000000000000000002',
              severity: 'high',
              target: 'apps/desktop/src',
              workspaceBoundary: 'workspace://local',
            },
            runId: 'run-001',
            sequence: 3,
            source: 'policy',
            timestamp: '2026-06-17T00:00:02.000Z',
            type: 'permission.requested',
            visibility: 'public',
          },
        ],
      },
    })

    renderAppShell(commandClient)

    expect(
      await within(screen.getByRole('region', { name: 'Activity' })).findByText(
        '01HZ0000000000000000000002',
      ),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))
    const details = await screen.findByRole('region', { name: 'Run event details' })

    expect(within(details).getAllByText('Write files')).not.toHaveLength(0)
    expect(within(details).getByText('Pending approval')).toBeInTheDocument()
    expect(within(details).queryByText('Approved')).not.toBeInTheDocument()
  })
})
