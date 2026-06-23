import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient, mockJyowoProject } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import { SidebarNav } from './SidebarNav'

const routerMock = vi.hoisted(() => ({
  navigate: vi.fn(
    async ({ search, to }: { search?: Record<string, string | undefined>; to: string }) => {
      const nextSearch = search
        ? `?${new URLSearchParams(
            Object.entries(search).filter(
              (entry): entry is [string, string] => typeof entry[1] === 'string',
            ),
          ).toString()}`
        : ''
      window.history.pushState(null, '', `${to}${nextSearch}`)
    },
  ),
}))

function renderSidebarNav(commandClient: CommandClient = createMockCommandClient()) {
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

  return render(<SidebarNav />, { wrapper: Wrapper })
}

function runtimeConversationClient() {
  const conversations = [
    {
      id: 'conversation-runtime-001',
      isEmpty: false,
      lastMessagePreview: 'Use the local journal',
      title: 'Runtime session',
      updatedAt: '2026-06-17T00:00:00.000Z',
    },
    {
      id: 'conversation-runtime-002',
      isEmpty: false,
      lastMessagePreview: 'Auth runtime preview',
      title: 'Auth runtime',
      updatedAt: '2026-06-17T00:00:01.000Z',
    },
  ]
  return {
    ...createMockCommandClient({
      projects: mockJyowoProject,
      contextSnapshot: {
        activeArtifact: null,
        decisions: [],
        files: [],
        nextActions: ['Continue the conversation'],
        path: '/Users/goya/Repo/Git/Jyowo',
        project: 'Jyowo',
      },
    }),
    async deleteConversation(conversationId: string) {
      const index = conversations.findIndex((conversation) => conversation.id === conversationId)
      if (index >= 0) {
        conversations.splice(index, 1)
      }
      return { conversationId, status: 'deleted' as const }
    },
    async listConversations() {
      return { conversations: [...conversations] }
    },
  } satisfies CommandClient
}

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerMock.navigate,
  useRouterState: ({
    select,
  }: {
    select: (state: {
      location: { pathname: string; search: { conversationId?: string } }
    }) => string | undefined
  }) => {
    const search = new URLSearchParams(window.location.search)

    return select({
      location: {
        pathname: window.location.pathname,
        search: { conversationId: search.get('conversationId') ?? undefined },
      },
    })
  },
}))

describe('SidebarNav', () => {
  beforeEach(async () => {
    routerMock.navigate.mockClear()
    await appI18n.changeLanguage('en-US')
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().setInspectorOpen(true)
      uiStore.getState().clearActiveRun()
    })
  })

  it('renders workspace navigation without search or workspace summary capsules', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-runtime-001')
    renderSidebarNav(runtimeConversationClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(within(navigation).getByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'Switch project' })).toBeInTheDocument()
    expect(within(navigation).queryByRole('searchbox', { name: 'Search' })).not.toBeInTheDocument()
    expect(within(navigation).getByText('Recent conversations')).toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'New conversation' })).toBeInTheDocument()
    expect(
      (await within(navigation).findByText('Runtime session')).closest('button'),
    ).toHaveAttribute('aria-current', 'page')
    expect(
      within(navigation).getByRole('button', { name: 'Delete Runtime session' }),
    ).toBeInTheDocument()
    expect(within(navigation).queryByText('Build the desktop foundation')).not.toBeInTheDocument()
    expect(within(navigation).queryByTestId('sidebar-bottom-navigation')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Home')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Artifacts')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Settings')).not.toBeInTheDocument()
    expect(within(navigation).queryByText(['Jane', 'Doe'].join(' '))).not.toBeInTheDocument()
    expect(within(navigation).queryByText(['Design', 'sandbox'].join(' '))).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Runs')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('MCP')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Memory')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Evals')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Models')).not.toBeInTheDocument()
  })

  it('routes selected conversation ids from real sidebar data', async () => {
    renderSidebarNav(runtimeConversationClient())

    fireEvent.click(
      (await screen.findByText('Auth runtime')).closest('button') as HTMLButtonElement,
    )

    expect(routerMock.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'conversation-runtime-002' },
      to: '/',
    })
  })

  it('localizes runtime default empty conversation labels', async () => {
    await appI18n.changeLanguage('zh-CN')
    renderSidebarNav(
      createMockCommandClient({
        projects: mockJyowoProject,
        conversations: {
          conversations: [
            {
              id: 'conversation-empty-001',
              isEmpty: true,
              lastMessagePreview: 'Start from the composer when ready.',
              title: 'New conversation',
              updatedAt: '2026-06-17T00:00:00.000Z',
            },
          ],
        },
      }),
    )

    const navigation = screen.getByRole('navigation', { name: '工作区' })

    expect(await within(navigation).findByText('新建对话')).toBeInTheDocument()
    expect(within(navigation).getByText('从输入框开始。')).toBeInTheDocument()
    expect(within(navigation).queryByText('New conversation')).not.toBeInTheDocument()
    expect(
      within(navigation).queryByText('Start from the composer when ready.'),
    ).not.toBeInTheDocument()
  })

  it('keeps real conversation labels that match runtime default text', async () => {
    await appI18n.changeLanguage('zh-CN')
    renderSidebarNav(
      createMockCommandClient({
        projects: mockJyowoProject,
        conversations: {
          conversations: [
            {
              id: 'conversation-real-001',
              isEmpty: false,
              lastMessagePreview: 'Start from the composer when ready.',
              title: 'New conversation',
              updatedAt: '2026-06-17T00:00:00.000Z',
            },
          ],
        },
      }),
    )

    const navigation = screen.getByRole('navigation', { name: '工作区' })

    expect(await within(navigation).findByText('New conversation')).toBeInTheDocument()
    expect(within(navigation).getByText('Start from the composer when ready.')).toBeInTheDocument()
    expect(within(navigation).queryByText('从输入框开始。')).not.toBeInTheDocument()
  })

  it('does not expose activity drilldown from the command palette', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })

    expect(screen.queryByRole('option', { name: 'View activity' })).not.toBeInTheDocument()
  })

  it('routes settings from the command palette', () => {
    renderSidebarNav(runtimeConversationClient())

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Settings' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/settings' })
  })

  it('does not expose skills as a standalone sidebar destination', () => {
    renderSidebarNav()

    expect(screen.queryByRole('button', { name: 'Skills' })).not.toBeInTheDocument()
  })

  it('creates a runtime conversation before routing from the command palette', async () => {
    window.history.pushState(null, '', '/settings')
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-001',
        runId: 'run-001',
      })
    })
    const createConversation = vi.fn(async () => ({
      conversation: {
        id: 'conversation-created-001',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    }))

    renderSidebarNav({
      ...createMockCommandClient({ projects: mockJyowoProject }),
      createConversation,
    })

    await screen.findByText('Jyowo')
    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createConversation).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(routerMock.navigate).toHaveBeenCalledWith({
        search: {
          conversationId: 'conversation-created-001',
        },
        to: '/',
      })
    })
    expect(uiStore.getState().activeRunConversationId).toBe('conversation-001')
    expect(uiStore.getState().activeRunId).toBe('run-001')
  })

  it('creates a runtime conversation from the recent conversation new action', async () => {
    window.history.pushState(null, '', '/settings')
    const createConversation = vi.fn(async () => ({
      conversation: {
        id: 'conversation-created-002',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    }))

    renderSidebarNav({
      ...runtimeConversationClient(),
      createConversation,
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createConversation).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(routerMock.navigate).toHaveBeenCalledWith({
        search: {
          conversationId: 'conversation-created-002',
        },
        to: '/',
      })
    })
  })

  it('shows a create conversation failure from the recent conversation new action', async () => {
    const createConversation = vi.fn(async () => {
      throw new Error('conversation create failed: session event stream does not start')
    })

    renderSidebarNav({
      ...createMockCommandClient({ projects: mockJyowoProject }),
      createConversation,
      listConversations: vi.fn(async () => ({ conversations: [] })),
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    expect(
      await screen.findByText('conversation create failed: session event stream does not start'),
    ).toBeInTheDocument()
    expect(routerMock.navigate).not.toHaveBeenCalledWith({
      search: expect.anything(),
      to: '/',
    })
  })

  it('routes to a newly created conversation before the refreshed list finishes', async () => {
    let listCallCount = 0
    const createConversation = vi.fn(async () => ({
      conversation: {
        id: 'conversation-created-fast-route',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    }))
    const listConversations = vi.fn(async () => {
      listCallCount += 1
      if (listCallCount > 1) {
        return new Promise<Awaited<ReturnType<CommandClient['listConversations']>>>(() => {})
      }
      return { conversations: [] }
    })

    renderSidebarNav({
      ...createMockCommandClient({ projects: mockJyowoProject }),
      createConversation,
      listConversations,
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(routerMock.navigate).toHaveBeenCalledWith({
        search: {
          conversationId: 'conversation-created-fast-route',
        },
        to: '/',
      })
    })
  })

  it('removes deleted conversations from the current sidebar list', async () => {
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'conversation-runtime-002',
        runId: 'run-runtime-002',
      })
    })

    renderSidebarNav(runtimeConversationClient())

    expect(await screen.findByText('Auth runtime')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Delete Auth runtime' }))

    await waitFor(() => {
      expect(screen.queryByText('Auth runtime')).not.toBeInTheDocument()
    })
    expect(uiStore.getState().activeRunsByConversation['conversation-runtime-002']).toBeUndefined()
  })

  it('deletes conversations through the command client before refreshing the sidebar list', async () => {
    const deleteConversation = vi.fn(async () => ({
      conversationId: 'conversation-runtime-002',
      status: 'deleted' as const,
    }))
    const listConversations = vi
      .fn()
      .mockResolvedValueOnce({
        conversations: [
          {
            id: 'conversation-runtime-001',
            isEmpty: false,
            lastMessagePreview: 'Use the local journal',
            title: 'Runtime session',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
          {
            id: 'conversation-runtime-002',
            isEmpty: false,
            lastMessagePreview: 'Auth runtime preview',
            title: 'Auth runtime',
            updatedAt: '2026-06-17T00:00:01.000Z',
          },
        ],
      })
      .mockResolvedValue({
        conversations: [
          {
            id: 'conversation-runtime-001',
            isEmpty: false,
            lastMessagePreview: 'Use the local journal',
            title: 'Runtime session',
            updatedAt: '2026-06-17T00:00:00.000Z',
          },
        ],
      })
    const commandClient = {
      ...createMockCommandClient({ projects: mockJyowoProject }),
      deleteConversation,
      listConversations,
    }

    renderSidebarNav(commandClient)

    expect(await screen.findByText('Auth runtime')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Delete Auth runtime' }))

    await waitFor(() => {
      expect(deleteConversation).toHaveBeenCalledWith('conversation-runtime-002')
    })
    await waitFor(() => {
      expect(listConversations).toHaveBeenCalledTimes(2)
    })
    expect(screen.queryByText('Auth runtime')).not.toBeInTheDocument()
  })

  it('routes evals from the command palette', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Open evals' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/evals' })
  })

  it('does not render a sidebar skills entry on the legacy skills route', () => {
    window.history.pushState(null, '', '/skills')

    renderSidebarNav(runtimeConversationClient())

    expect(screen.queryByRole('button', { name: 'Skills' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Conversations' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Agents' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Tools' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Settings' })).not.toBeInTheDocument()
  })

  it('does not expose skills on the settings route', () => {
    window.history.pushState(null, '', '/settings')

    renderSidebarNav()

    expect(screen.queryByRole('button', { name: 'Skills' })).not.toBeInTheDocument()
  })

  it('renders a collapsed sidebar from local UI state', () => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(true)
    })

    renderSidebarNav(runtimeConversationClient())

    expect(screen.getByRole('navigation', { name: 'Workspace' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Switch project' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Skills' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Agents' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Settings' })).not.toBeInTheDocument()
    expect(screen.queryByText('Recent conversations')).not.toBeInTheDocument()
  })

  it('collapses the expanded sidebar from the navigation control', () => {
    renderSidebarNav()

    fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))

    expect(screen.getByRole('navigation', { name: 'Workspace' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
  })
})
