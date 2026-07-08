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
      updatedAt: new Date(Date.now() - 60 * 60 * 1000).toISOString(),
    },
    {
      id: 'conversation-runtime-002',
      isEmpty: false,
      lastMessagePreview: 'Auth runtime preview',
      title: 'Auth runtime',
      updatedAt: new Date(Date.now() - 4 * 24 * 60 * 60 * 1000).toISOString(),
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
    window.localStorage.clear()
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

  it('renders a compact project-tree sidebar with project controls and relative times', async () => {
    window.history.pushState(null, '', '/?conversationId=conversation-runtime-001')
    renderSidebarNav(runtimeConversationClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(within(navigation).getByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument()
    expect(
      within(navigation).queryByRole('button', { name: 'Switch project' }),
    ).not.toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'New conversation' })).toBeInTheDocument()
    expect(within(navigation).queryByRole('button', { name: 'Search' })).not.toBeInTheDocument()
    expect(within(navigation).queryByRole('button', { name: 'Scheduled' })).not.toBeInTheDocument()
    expect(within(navigation).queryByRole('button', { name: 'Plugins' })).not.toBeInTheDocument()
    expect(within(navigation).getByText('Pinned')).toBeInTheDocument()
    expect(within(navigation).getByText('Projects')).toBeInTheDocument()
    expect(await within(navigation).findByText('Jyowo')).toBeInTheDocument()
    expect(within(navigation).getByText('1 h')).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'Project actions for Jyowo' }),
    ).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'New conversation in Jyowo' }),
    ).toBeInTheDocument()
    expect(within(navigation).getByRole('button', { name: 'Pin Runtime session' })).toHaveClass(
      'opacity-0',
      'group-hover:opacity-100',
    )
    expect(
      (await within(navigation).findByText('Runtime session')).closest('button'),
    ).toHaveAttribute('aria-current', 'page')
    expect(
      within(navigation).getByText('Runtime session').closest('[data-sidebar-row]'),
    ).toHaveAttribute('data-depth', 'conversation')
    const projectRow = within(navigation).getByText('Jyowo').closest('[data-sidebar-row]')
    expect(projectRow).toHaveAttribute('data-active', 'true')
    expect(projectRow).toHaveClass('grid-cols-[minmax(0,1fr)_auto]')
    expect(
      within(navigation).getByText('Runtime session').closest('[data-sidebar-row]'),
    ).toHaveClass('grid-cols-[minmax(0,1fr)_auto]')
    expect(within(navigation).getByRole('button', { name: 'Delete Runtime session' })).toHaveClass(
      'opacity-0',
      'group-hover:opacity-100',
    )
    expect(within(navigation).queryByText('Build the desktop foundation')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Recent conversations')).not.toBeInTheDocument()
    expect(
      within(navigation).queryByRole('searchbox', { name: 'Search conversations' }),
    ).not.toBeInTheDocument()
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

  it('shows conversations grouped under multiple projects with per-project actions', async () => {
    renderSidebarNav(projectConversationGroupsClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('alpha')).toBeInTheDocument()
    expect(within(navigation).getByText('beta')).toBeInTheDocument()
    expect(within(navigation).getByText('Sidebar redesign')).toBeInTheDocument()
    expect(within(navigation).getByText('Release checklist')).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'New conversation in alpha' }),
    ).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'New conversation in beta' }),
    ).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'Project actions for alpha' }),
    ).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'Project actions for beta' }),
    ).toBeInTheDocument()
  })

  it('collapses and expands project conversation children from the project row disclosure', async () => {
    renderSidebarNav(projectConversationGroupsClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('Sidebar redesign')).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse alpha' }))

    expect(within(navigation).queryByText('Sidebar redesign')).not.toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Expand alpha' }))

    expect(await within(navigation).findByText('Sidebar redesign')).toBeInTheDocument()
  })

  it('does not render empty labels under projects without conversations', async () => {
    renderSidebarNav(
      projectConversationGroupsClient({
        async listProjectConversationGroups() {
          return {
            activePath: '/repo/alpha',
            groups: [
              {
                project: {
                  path: '/repo/alpha',
                  name: 'alpha',
                  lastOpenedAt: '2026-07-08T07:00:00.000Z',
                },
                conversations: [],
              },
              {
                project: {
                  path: '/repo/beta',
                  name: 'beta',
                  lastOpenedAt: '2026-07-07T07:00:00.000Z',
                },
                conversations: [],
              },
            ],
          }
        },
      }),
    )

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('alpha')).toBeInTheDocument()
    expect(within(navigation).getByText('beta')).toBeInTheDocument()
    expect(within(navigation).getAllByText('No conversations')).toHaveLength(1)
  })

  it('renders default conversations in a collapsible conversation section when projects exist', async () => {
    const createDefaultConversation = vi.fn(async () => ({
      conversation: {
        id: 'projectless-created-001',
        isEmpty: false,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'Projectless draft',
        updatedAt: '2026-07-08T07:03:00.000Z',
      },
    }))
    renderSidebarNav(projectConversationGroupsClient({ createDefaultConversation }))

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(
      await within(navigation).findByRole('button', { name: 'Collapse Conversations' }),
    ).toBeInTheDocument()
    expect(within(navigation).getByText('No conversations')).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createDefaultConversation).toHaveBeenCalledTimes(1)
    })
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'projectless-created-001' },
      to: '/',
    })
    expect(await within(navigation).findByText('Projectless draft')).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse Conversations' }))

    expect(within(navigation).queryByText('Projectless draft')).not.toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Expand Conversations' }))

    expect(await within(navigation).findByText('Projectless draft')).toBeInTheDocument()
  })

  it('bubbles running indicators from conversation rows to collapsed parents', async () => {
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'alpha-001',
        runId: 'run-alpha-001',
      })
    })
    renderSidebarNav(projectConversationGroupsClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(
      await within(navigation).findByLabelText('Sidebar redesign is running'),
    ).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse alpha' }))

    expect(
      within(navigation).queryByLabelText('Sidebar redesign is running'),
    ).not.toBeInTheDocument()
    expect(
      within(navigation).getByLabelText('alpha has a running conversation'),
    ).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse Projects' }))

    expect(within(navigation).queryByText('alpha')).not.toBeInTheDocument()
    expect(
      within(navigation).getByLabelText('Projects has a running conversation'),
    ).toBeInTheDocument()
  })

  it('bubbles pinned running conversations to collapsed project parents', async () => {
    window.localStorage.setItem('jyowo.sidebar.pinnedConversationIds', '["alpha-001"]')
    act(() => {
      uiStore.getState().setActiveRun({
        conversationId: 'alpha-001',
        runId: 'run-alpha-001',
      })
    })
    renderSidebarNav(projectConversationGroupsClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(
      await within(navigation).findByRole('button', { name: 'Unpin Sidebar redesign' }),
    ).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse alpha' }))

    expect(
      within(navigation).getByLabelText('alpha has a running conversation'),
    ).toBeInTheDocument()

    fireEvent.click(within(navigation).getByRole('button', { name: 'Collapse Projects' }))

    expect(
      within(navigation).getByLabelText('Projects has a running conversation'),
    ).toBeInTheDocument()
  })

  it('switches to the target project before creating from a project row', async () => {
    const switchProject = vi.fn(async (path: string) => ({
      project: {
        path,
        name: 'beta',
        lastOpenedAt: '2026-07-08T07:02:00.000Z',
      },
    }))
    const createProjectConversation = vi.fn(async () => ({
      conversation: {
        id: 'beta-created-001',
        isEmpty: true,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'New conversation',
        updatedAt: '2026-07-08T07:03:00.000Z',
      },
    }))
    renderSidebarNav(projectConversationGroupsClient({ createProjectConversation, switchProject }))

    fireEvent.click(await screen.findByRole('button', { name: 'New conversation in beta' }))

    await waitFor(() => {
      expect(switchProject).toHaveBeenCalledWith('/repo/beta')
    })
    await waitFor(() => {
      expect(createProjectConversation).toHaveBeenCalledWith('/repo/beta')
    })
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'beta-created-001' },
      to: '/',
    })
  })

  it('opens project actions for ordering and removal only', async () => {
    const moveProject = vi.fn(async () => ({
      activePath: '/repo/alpha',
      projects: [],
    }))
    renderSidebarNav(projectConversationGroupsClient({ moveProject }))

    fireEvent.pointerDown(await screen.findByRole('button', { name: 'Project actions for beta' }))

    expect(screen.queryByRole('menuitem', { name: 'Switch to beta' })).not.toBeInTheDocument()
    expect(screen.queryByRole('menuitem', { name: 'New conversation' })).not.toBeInTheDocument()
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Move up' }))

    await waitFor(() => {
      expect(moveProject).toHaveBeenCalledWith('/repo/beta', 'up')
    })
  })

  it('opens the remove project confirmation from the overflow menu', async () => {
    renderSidebarNav(projectConversationGroupsClient())

    fireEvent.pointerDown(await screen.findByRole('button', { name: 'Project actions for beta' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Remove project' }))

    expect(await screen.findByRole('dialog')).toHaveTextContent(
      'Remove beta from Jyowo. The project folder stays on disk.',
    )
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

    const releaseConversationButton = (await screen.findByText('Release checklist')).closest(
      'button',
    )
    expect(releaseConversationButton).toBeInstanceOf(HTMLButtonElement)
    fireEvent.click(releaseConversationButton as HTMLButtonElement)

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

    const releaseConversationButton = (await screen.findByText('Release checklist')).closest(
      'button',
    )
    expect(releaseConversationButton).toBeInstanceOf(HTMLButtonElement)
    fireEvent.click(releaseConversationButton as HTMLButtonElement)

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

  it('pins and unpins project conversations in the pinned section', async () => {
    renderSidebarNav(projectConversationGroupsClient())

    fireEvent.click(await screen.findByRole('button', { name: 'Pin Sidebar redesign' }))

    expect(
      await screen.findByRole('button', { name: 'Unpin Sidebar redesign' }),
    ).toBeInTheDocument()
    expect(window.localStorage.getItem('jyowo.sidebar.pinnedConversationIds')).toContain(
      'alpha-001',
    )

    fireEvent.click(screen.getByRole('button', { name: 'Unpin Sidebar redesign' }))

    expect(await screen.findByRole('button', { name: 'Pin Sidebar redesign' })).toBeInTheDocument()
    expect(window.localStorage.getItem('jyowo.sidebar.pinnedConversationIds')).toBe('[]')
  })

  it('keeps pinned ids that are outside the current project conversation result', async () => {
    window.localStorage.setItem('jyowo.sidebar.pinnedConversationIds', '["older-pinned-001"]')

    renderSidebarNav(projectConversationGroupsClient())

    expect(await screen.findByText('Sidebar redesign')).toBeInTheDocument()
    expect(window.localStorage.getItem('jyowo.sidebar.pinnedConversationIds')).toBe(
      '["older-pinned-001"]',
    )
  })

  it('lists and creates default conversations without an active project', async () => {
    const createDefaultConversation = vi.fn(async () => ({
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
      createDefaultConversation,
    })

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('No workspace conversation')).toBeInTheDocument()
    fireEvent.click(within(navigation).getAllByRole('button', { name: 'New conversation' })[0])

    await waitFor(() => {
      expect(createDefaultConversation).toHaveBeenCalledTimes(1)
    })
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'conversation-no-workspace-created' },
      to: '/',
    })
  })

  it('shows global new conversations in the conversation section, not under projects', async () => {
    const createDefaultConversation = vi.fn(async () => ({
      conversation: {
        id: 'projectless-created-001',
        isEmpty: false,
        lastMessagePreview: 'Start from the composer when ready.',
        title: 'Projectless draft',
        updatedAt: '2026-07-08T07:03:00.000Z',
      },
    }))
    renderSidebarNav(projectConversationGroupsClient({ createDefaultConversation }))

    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createDefaultConversation).toHaveBeenCalledTimes(1)
    })
    expect(routerSpy.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'projectless-created-001' },
      to: '/',
    })
    expect(await screen.findByText('Projectless draft')).toBeInTheDocument()
    expect(screen.getByText('alpha')).toBeInTheDocument()
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
              updatedAt: new Date(Date.now() - 60 * 60 * 1000).toISOString(),
            },
          ],
        },
      }),
    )

    const navigation = screen.getByRole('navigation', { name: '工作区' })

    expect(await within(navigation).findByText('Jyowo')).toBeInTheDocument()
    expect((await within(navigation).findAllByText('新建对话')).length).toBeGreaterThan(0)
    expect(within(navigation).getByText('1 小时')).toBeInTheDocument()
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

  it('does not expose scheduled or plugin action rows in the sidebar', async () => {
    renderSidebarNav(runtimeConversationClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(await within(navigation).findByText('Jyowo')).toBeInTheDocument()
    expect(within(navigation).queryByRole('button', { name: 'Scheduled' })).not.toBeInTheDocument()
    expect(within(navigation).queryByRole('button', { name: 'Plugins' })).not.toBeInTheDocument()
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
    const createDefaultConversation = vi.fn(async () => ({
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
      createDefaultConversation,
    })

    await screen.findByText('Jyowo')
    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createDefaultConversation).toHaveBeenCalledTimes(1)
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
    const createDefaultConversation = vi.fn(async () => ({
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
      createDefaultConversation,
    })

    await screen.findByText('Jyowo')
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation' }))

    await waitFor(() => {
      expect(createDefaultConversation).toHaveBeenCalledTimes(1)
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
    const createDefaultConversation = vi.fn(async () => {
      throw new Error('conversation create failed: session event stream does not start')
    })

    renderSidebarNav({
      ...createTestCommandClient({ projects: testJyowoProject }),
      createDefaultConversation,
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
    const createDefaultConversation = vi.fn(async () => ({
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
      createDefaultConversation,
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

  it('shows delete actions for inactive project conversations', async () => {
    const deleteProjectConversation = vi.fn(async () => ({
      conversationId: 'beta-001',
      status: 'deleted' as const,
    }))

    renderSidebarNav(projectConversationGroupsClient({ deleteProjectConversation }))

    expect(await screen.findByText('Release checklist')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Delete Release checklist' }))

    await waitFor(() => {
      expect(deleteProjectConversation).toHaveBeenCalledWith('/repo/beta', 'beta-001')
    })
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

  it('does not render a sidebar skills entry on the old skills route', () => {
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
