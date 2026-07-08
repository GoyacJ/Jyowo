import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient, testJyowoProject } from '@/testing/command-client'
import { SidebarNav } from './SidebarNav'

const routerSpy = vi.hoisted(() => ({
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

const pickProjectDirectoryMock = vi.hoisted(() => vi.fn())

vi.mock('@/shared/tauri/file-dialog', () => ({
  pickProjectDirectory: pickProjectDirectoryMock,
}))

function renderSidebarNav(commandClient: CommandClient = createTestCommandClient()) {
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
    ...createTestCommandClient({
      projects: testJyowoProject,
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
    async listProjectConversationGroups() {
      return {
        activePath: '/Users/goya/Repo/Git/Jyowo',
        groups: [
          {
            project: {
              path: '/Users/goya/Repo/Git/Jyowo',
              name: 'Jyowo',
              lastOpenedAt: '2026-06-17T00:00:00.000Z',
            },
            conversations: [...conversations],
          },
        ],
      }
    },
  } satisfies CommandClient
}

function projectConversationGroupsClient(
  overrides: Partial<CommandClient> = {},
  activePath: string | null = '/repo/alpha',
) {
  const projects = {
    activePath,
    projects: [
      {
        path: '/repo/alpha',
        name: 'alpha',
        lastOpenedAt: '2026-07-08T07:00:00.000Z',
      },
      {
        path: '/repo/beta',
        name: 'beta',
        lastOpenedAt: '2026-07-07T07:00:00.000Z',
      },
    ],
  }
  return {
    ...createTestCommandClient({ projects }),
    async listProjectConversationGroups() {
      return {
        activePath,
        groups: [
          {
            project: projects.projects[0],
            conversations: [
              {
                id: 'alpha-001',
                isEmpty: false,
                lastMessagePreview: 'Review the left menu',
                title: 'Sidebar redesign',
                updatedAt: '2026-07-08T07:01:00.000Z',
              },
            ],
          },
          {
            project: projects.projects[1],
            conversations: [
              {
                id: 'beta-001',
                isEmpty: false,
                lastMessagePreview: 'Deploy preview',
                title: 'Release checklist',
                updatedAt: '2026-07-07T07:01:00.000Z',
              },
            ],
          },
        ],
      }
    },
    ...overrides,
  } satisfies CommandClient
}

vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerSpy.navigate,
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
    routerSpy.navigate.mockClear()
    pickProjectDirectoryMock.mockReset()
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
    expect(within(navigation).queryByRole('button', { name: 'Switch project' })).not.toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'New project' })).toBeInTheDocument()
    expect(within(navigation).getByRole('searchbox', { name: 'Search conversations' })).toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'New conversation' })).toBeInTheDocument()
    expect(await within(navigation).findByText('Jyowo')).toBeInTheDocument()
    expect(
      (await within(navigation).findByText('Runtime session')).closest('button'),
    ).toHaveAttribute('aria-current', 'page')
    expect(within(navigation).getByRole('button', { name: 'Delete Runtime session' })).toHaveClass(
      'opacity-0',
      'group-hover:opacity-100',
    )
    expect(within(navigation).queryByText('Build the desktop foundation')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Recent conversations')).not.toBeInTheDocument()
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

  it('shows conversations grouped under multiple projects and filters locally', async () => {
    renderSidebarNav(projectConversationGroupsClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('alpha')).toBeInTheDocument()
    expect(within(navigation).getByText('beta')).toBeInTheDocument()
    expect(within(navigation).getByText('Sidebar redesign')).toBeInTheDocument()
    expect(within(navigation).getByText('Release checklist')).toBeInTheDocument()

    fireEvent.change(within(navigation).getByRole('searchbox', { name: 'Search conversations' }), {
      target: { value: 'deploy' },
    })

    expect(within(navigation).getByText('Release checklist')).toBeInTheDocument()
    expect(within(navigation).queryByText('Sidebar redesign')).not.toBeInTheDocument()
  })

  it('switches project before opening a conversation from another project', async () => {
    const switchProject = vi.fn(async (path: string) => ({
      project: {
        path,
        name: 'beta',
        lastOpenedAt: '2026-07-08T07:02:00.000Z',
      },
    }))
    renderSidebarNav(projectConversationGroupsClient({ switchProject }))

    fireEvent.click((await screen.findByText('Release checklist')).closest('button')!)

    await waitFor(() => {
      expect(switchProject).toHaveBeenCalledWith('/repo/beta')
    })
    await waitFor(() => {
      expect(routerSpy.navigate).toHaveBeenLastCalledWith({
        search: { conversationId: 'beta-001' },
        to: '/',
      })
    })
  })

  it('switches project before opening a conversation when no project is active', async () => {
    const switchProject = vi.fn(async (path: string) => ({
      project: {
        path,
        name: 'beta',
        lastOpenedAt: '2026-07-08T07:02:00.000Z',
      },
    }))
    renderSidebarNav(projectConversationGroupsClient({ switchProject }, null))

    fireEvent.click((await screen.findByText('Release checklist')).closest('button')!)

    await waitFor(() => {
      expect(switchProject).toHaveBeenCalledWith('/repo/beta')
    })
    await waitFor(() => {
      expect(routerSpy.navigate).toHaveBeenLastCalledWith({
        search: { conversationId: 'beta-001' },
        to: '/',
      })
    })
  })

  it('routes selected conversation ids from real sidebar data', async () => {
    renderSidebarNav(runtimeConversationClient())

    fireEvent.click(
      (await screen.findByText('Auth runtime')).closest('button') as HTMLButtonElement,
    )

    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'conversation-runtime-002' },
      to: '/',
    })
  })

  it('lists and creates runtime conversations without an active project', async () => {
    const createConversation = vi.fn(async () => ({
      conversation: {
        id: 'conversation-no-workspace-created',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-06-17T00:00:00.000Z',
      },
    }))

    renderSidebarNav({
      ...createTestCommandClient({
        projects: { activePath: null, projects: [] },
        conversations: {
          conversations: [
            {
              id: 'conversation-no-workspace-existing',
              isEmpty: false,
              lastMessagePreview: 'Global runtime session',
              title: 'No workspace conversation',
              updatedAt: '2026-06-17T00:00:00.000Z',
            },
          ],
        },
      }),
      createConversation,
    })

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('No workspace conversation')).toBeInTheDocument()
    fireEvent.click(within(navigation).getAllByRole('button', { name: 'New conversation' })[0])

    await waitFor(() => {
      expect(createConversation).toHaveBeenCalledTimes(1)
    })
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'conversation-no-workspace-created' },
      to: '/',
    })
  })

  it('localizes runtime default empty conversation labels', async () => {
    await appI18n.changeLanguage('zh-CN')
    renderSidebarNav(
      createTestCommandClient({
        projects: testJyowoProject,
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

    expect(await within(navigation).findByText('Jyowo')).toBeInTheDocument()
    expect((await within(navigation).findAllByText('新建对话')).length).toBeGreaterThan(0)
    expect(within(navigation).getByText('从输入框开始。')).toBeInTheDocument()
    expect(within(navigation).queryByText('New conversation')).not.toBeInTheDocument()
    expect(
      within(navigation).queryByText('Start from the composer when ready.'),
    ).not.toBeInTheDocument()
  })

  it('keeps real conversation labels that match runtime default text', async () => {
    await appI18n.changeLanguage('zh-CN')
    renderSidebarNav(
      createTestCommandClient({
        projects: testJyowoProject,
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

    expect(routerSpy.navigate).toHaveBeenCalledWith({ to: '/settings' })
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
      ...createTestCommandClient({ projects: testJyowoProject }),
      createConversation,
    })

    await screen.findByText('Jyowo')
    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createConversation).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(routerSpy.navigate).toHaveBeenCalledWith({
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
      expect(routerSpy.navigate).toHaveBeenCalledWith({
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
      ...createTestCommandClient({ projects: testJyowoProject }),
      createConversation,
      listConversations: vi.fn(async () => ({ conversations: [] })),
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    expect(
      await screen.findByText('conversation create failed: session event stream does not start'),
    ).toBeInTheDocument()
    expect(routerSpy.navigate).not.toHaveBeenCalledWith({
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
      ...createTestCommandClient({ projects: testJyowoProject }),
      createConversation,
      listConversations,
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(routerSpy.navigate).toHaveBeenCalledWith({
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
    const listProjectConversationGroups = vi
      .fn()
      .mockResolvedValueOnce({
        activePath: '/Users/goya/Repo/Git/Jyowo',
        groups: [
          {
            project: testJyowoProject.projects[0],
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
          },
        ],
      })
      .mockResolvedValue({
        activePath: '/Users/goya/Repo/Git/Jyowo',
        groups: [
          {
            project: testJyowoProject.projects[0],
            conversations: [
              {
                id: 'conversation-runtime-001',
                isEmpty: false,
                lastMessagePreview: 'Use the local journal',
                title: 'Runtime session',
                updatedAt: '2026-06-17T00:00:00.000Z',
              },
            ],
          },
        ],
      })
    const commandClient = {
      ...createTestCommandClient({ projects: testJyowoProject }),
      deleteConversation,
      listProjectConversationGroups,
    }

    renderSidebarNav(commandClient)

    expect(await screen.findByText('Auth runtime')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Delete Auth runtime' }))

    await waitFor(() => {
      expect(deleteConversation).toHaveBeenCalledWith('conversation-runtime-002')
    })
    await waitFor(() => {
      expect(listProjectConversationGroups).toHaveBeenCalledTimes(2)
    })
    expect(screen.queryByText('Auth runtime')).not.toBeInTheDocument()
  })

  it('shows delete command errors in the conversation list', async () => {
    const deleteConversation = vi.fn(async () => {
      throw new Error('conversation not found: conversation-runtime-002')
    })
    const commandClient = {
      ...runtimeConversationClient(),
      deleteConversation,
    }

    renderSidebarNav(commandClient)

    expect(await screen.findByText('Auth runtime')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Delete Auth runtime' }))

    expect(
      await screen.findByText('conversation not found: conversation-runtime-002'),
    ).toBeInTheDocument()
    expect(screen.getByText('Auth runtime')).toBeInTheDocument()
  })

  it('adds a project from the sidebar action', async () => {
    pickProjectDirectoryMock.mockResolvedValue('/Users/goya/Repo/Git/NextApp')
    const addProject = vi.fn(async (path: string) => ({
      project: {
        lastOpenedAt: '2026-07-08T07:00:00.000Z',
        name: 'NextApp',
        path,
      },
    }))
    const commandClient = {
      ...createTestCommandClient({ projects: testJyowoProject }),
      addProject,
    }

    renderSidebarNav(commandClient)

    expect(await screen.findByText('Jyowo')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'New project' }))

    await waitFor(() => {
      expect(addProject).toHaveBeenCalledWith('/Users/goya/Repo/Git/NextApp')
    })
  })

  it('routes evals from the command palette', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Open evals' }))

    expect(routerSpy.navigate).toHaveBeenCalledWith({ to: '/evals' })
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
    expect(screen.getByRole('button', { name: 'New conversation' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New project' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Switch project' })).not.toBeInTheDocument()
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
