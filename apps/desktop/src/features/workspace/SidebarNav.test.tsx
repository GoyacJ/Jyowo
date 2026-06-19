import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { uiStore } from '@/shared/state/ui-store'
import { SidebarNav } from './SidebarNav'

const preferencesStoreMock = vi.hoisted(() => ({
  readUiPreferences: vi.fn(async () => ({
    theme: 'system' as const,
    sidebarCollapsed: false,
    chatComposerHeight: 160,
    contextPanelWidth: 320,
    lastSelectedWorkspaceRef: 'local:current',
  })),
  writeUiPreferences: vi.fn(async () => {}),
}))

const routerMock = vi.hoisted(() => ({
  navigate: vi.fn(async ({ to }: { to: string }) => {
    window.history.pushState(null, '', to)
  }),
}))

vi.mock('@/shared/local-store/ui-preferences-store', () => preferencesStoreMock)
vi.mock('@tanstack/react-router', async () => ({
  useNavigate: () => routerMock.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => string }) =>
    select({ location: { pathname: window.location.pathname } }),
}))

describe('SidebarNav', () => {
  beforeEach(() => {
    preferencesStoreMock.readUiPreferences.mockResolvedValue({
      theme: 'system',
      sidebarCollapsed: false,
      chatComposerHeight: 160,
      contextPanelWidth: 320,
      lastSelectedWorkspaceRef: 'local:current',
    })
    preferencesStoreMock.writeUiPreferences.mockClear()
    routerMock.navigate.mockClear()
    window.history.pushState(null, '', '/')
  })

  afterEach(() => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(false)
      uiStore.getState().setSelectedWorkspaceRef(null)
      uiStore.getState().setActivityRailExpanded(false)
      uiStore.getState().setActivityRailCollapsed(false)
      uiStore.getState().setInspectorOpen(true)
    })
  })

  it('renders workspace navigation with active conversation and local identity', () => {
    render(<SidebarNav />)

    const navigation = screen.getByRole('navigation', { name: 'Workspace' })

    expect(within(navigation).getByRole('searchbox', { name: 'Search' })).toBeInTheDocument()
    expect(within(navigation).getByText('Recent conversations')).toBeInTheDocument()
    expect(
      within(navigation).getByRole('button', { name: 'Build the desktop foundation' }),
    ).toHaveAttribute('aria-current', 'page')
    expect(within(navigation).getByText('Home')).toBeInTheDocument()
    expect(within(navigation).getByText('Conversations')).toBeInTheDocument()
    expect(within(navigation).getByText('Projects')).toBeInTheDocument()
    expect(within(navigation).getByText('Artifacts')).toBeInTheDocument()
    expect(within(navigation).getByText('Agents')).toBeInTheDocument()
    expect(within(navigation).getByText('Tools')).toBeInTheDocument()
    expect(within(navigation).getByText('Settings')).toBeInTheDocument()
    expect(within(navigation).getByText('Jane Doe')).toBeInTheDocument()
    expect(within(navigation).getByText('Local workspace')).toBeInTheDocument()
    expect(within(navigation).queryByText('Runs')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('MCP')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Memory')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Evals')).not.toBeInTheDocument()
    expect(within(navigation).queryByText('Models')).not.toBeInTheDocument()
  })

  it('hydrates and persists the selected local workspace reference', async () => {
    preferencesStoreMock.readUiPreferences.mockResolvedValue({
      theme: 'system',
      sidebarCollapsed: false,
      chatComposerHeight: 160,
      contextPanelWidth: 320,
      lastSelectedWorkspaceRef: 'local:design-sandbox',
    })

    render(<SidebarNav />)

    expect(
      await screen.findByRole('group', { name: 'Current workspace: Design sandbox' }),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Choose workspace' }))
    fireEvent.click(screen.getByRole('button', { name: 'Jyowo Current project' }))

    await waitFor(() => {
      expect(preferencesStoreMock.writeUiPreferences).toHaveBeenCalledWith({
        lastSelectedWorkspaceRef: 'local:current',
      })
    })
  })

  it('filters recent conversations from the sidebar search', () => {
    render(<SidebarNav />)

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search' }), {
      target: { value: 'auth' },
    })

    expect(screen.getByRole('button', { name: 'Refactor auth module' })).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'Build the desktop foundation' }),
    ).not.toBeInTheDocument()
  })

  it('runs command palette actions through sidebar UI state', () => {
    render(<SidebarNav />)

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'View activity' }))

    expect(uiStore.getState().activityRailExpanded).toBe(true)
    expect(uiStore.getState().activityRailCollapsed).toBe(false)
  })

  it('marks artifact and settings destinations from command palette actions', () => {
    render(<SidebarNav />)

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

    render(<SidebarNav />)

    fireEvent.click(screen.getByRole('button', { name: 'New conversation' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/' })
  })

  it('routes evals from the command palette', () => {
    render(<SidebarNav />)

    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    fireEvent.click(screen.getByRole('option', { name: 'Open evals' }))

    expect(routerMock.navigate).toHaveBeenCalledWith({ to: '/evals' })
  })

  it('renders a collapsed sidebar from local UI state', () => {
    act(() => {
      uiStore.getState().setSidebarCollapsed(true)
    })

    render(<SidebarNav />)

    expect(screen.getByRole('navigation', { name: 'Workspace' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    expect(screen.queryByText('Recent conversations')).not.toBeInTheDocument()
  })
})
