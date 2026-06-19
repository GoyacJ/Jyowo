import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
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
  return createMockCommandClient({
    contextSnapshot: {
      activeArtifact: null,
      decisions: [],
      files: [],
      nextActions: ['Continue the conversation'],
      path: '/Users/goya/Repo/Git/Jyowo',
      project: 'Jyowo',
    },
    conversations: {
      conversations: [
        {
          id: 'conversation-runtime-001',
          lastMessagePreview: 'Use the local journal',
          title: 'Runtime session',
          updatedAt: '2026-06-17T00:00:00.000Z',
        },
        {
          id: 'conversation-runtime-002',
          lastMessagePreview: 'Auth runtime preview',
          title: 'Auth runtime',
          updatedAt: '2026-06-17T00:00:01.000Z',
        },
      ],
    },
  })
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
  beforeEach(() => {
    routerMock.navigate.mockClear()
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().setActivityRailExpanded(false)
      uiStore.getState().setActivityRailCollapsed(false)
      uiStore.getState().setInspectorOpen(true)
    })
  })

  it('renders workspace navigation with active conversation and workspace summary', async () => {
    renderSidebarNav(runtimeConversationClient())

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(within(navigation).getByRole('searchbox', { name: 'Search' })).toBeInTheDocument()
    expect(within(navigation).getByText('Recent conversations')).toBeInTheDocument()
    expect(
      await within(navigation).findByRole('button', { name: /Runtime session/ }),
    ).toHaveAttribute('aria-current', 'page')
    expect(within(navigation).queryByText('Build the desktop foundation')).not.toBeInTheDocument()
    expect(within(navigation).getByText('Home')).toBeInTheDocument()
    expect(within(navigation).getByText('Conversations')).toBeInTheDocument()
    expect(within(navigation).getByText('Projects')).toBeInTheDocument()
    expect(within(navigation).getByText('Artifacts')).toBeInTheDocument()
    expect(within(navigation).getByText('Agents')).toBeInTheDocument()
    expect(within(navigation).getByText('Tools')).toBeInTheDocument()
    expect(within(navigation).getByText('Settings')).toBeInTheDocument()
    expect(await within(navigation).findAllByText('Jyowo')).not.toHaveLength(0)
    expect(within(navigation).getByText('/Users/goya/Repo/Git/Jyowo')).toBeInTheDocument()
    expect(within(navigation).queryByText(['Jane', 'Doe'].join(' '))).not.toBeInTheDocument()
    expect(within(navigation).queryByText(['Design', 'sandbox'].join(' '))).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Runs')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('MCP')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Memory')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Evals')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Models')).not.toBeInTheDocument()
  })

  it('filters recent conversations from the sidebar search', async () => {
    renderSidebarNav(runtimeConversationClient())

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search' }), {
      target: { value: 'auth' },
    })

    expect(await screen.findByRole('button', { name: /Auth runtime/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Runtime session/ })).not.toBeInTheDocument()
  })

  it('routes selected conversation ids from real sidebar data', async () => {
    renderSidebarNav(runtimeConversationClient())

    fireEvent.click(await screen.findByRole('button', { name: /Auth runtime/ }))

    expect(routerMock.navigate).toHaveBeenCalledWith({
      search: { conversationId: 'conversation-runtime-002' },
      to: '/',
    })
  })

  it('runs command palette actions through sidebar UI state', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'View activity' }))

    expect(uiStore.getState().activityRailExpanded).toBe(true)
    expect(uiStore.getState().activityRailCollapsed).toBe(false)
  })

  it('marks artifact and settings destinations from command palette actions', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Open artifact' }))

    expect(screen.getByRole('button', { name: 'Artifacts' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Artifacts' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/artifacts' })

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Settings' }))

    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute('data-active', 'true')
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute('aria-current', 'page')
    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/settings' })
  })

  it('routes new conversation to the conversation workspace before focusing composer', () => {
    window.history.pushState(null, '', '/settings')

    renderSidebarNav()

    fireEvent.click(screen.getByRole('button', { name: 'New conversation' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/' })
  })

  it('routes evals from the command palette', () => {
    renderSidebarNav()

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Open evals' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/evals' })
  })

  it('renders a collapsed sidebar from local UI state', () => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(true)
    })

    renderSidebarNav()

    expect(screen.getByRole('navigation', { name: 'Workspace' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    expect(screen.queryByText('Recent conversations')).not.toBeInTheDocument()
  })
})
