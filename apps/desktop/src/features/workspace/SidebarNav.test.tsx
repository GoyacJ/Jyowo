import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient } from '@/shared/tauri/commands'

import { SidebarNav } from './SidebarNav'

const mocks = vi.hoisted(() => ({
  commandClient: null as unknown as CommandClient,
  daemonClient: null as unknown as DaemonClient,
  navigate: vi.fn(),
  pickProjectDirectory: vi.fn(),
  selectedTaskId: undefined as string | undefined,
  pathname: '/',
}))

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => mocks.navigate,
  useRouterState: ({ select }: { select: (state: unknown) => unknown }) =>
    select({ location: { pathname: mocks.pathname, search: { taskId: mocks.selectedTaskId } } }),
}))

vi.mock('@/shared/tauri/react', () => ({
  useCommandClient: () => mocks.commandClient,
  useDaemonClient: () => mocks.daemonClient,
}))

vi.mock('@/shared/tauri/file-dialog', () => ({
  pickProjectDirectory: mocks.pickProjectDirectory,
}))

describe('SidebarNav task navigation', () => {
  beforeEach(() => {
    mocks.navigate.mockReset().mockResolvedValue(undefined)
    mocks.pickProjectDirectory.mockReset().mockResolvedValue(null)
    mocks.selectedTaskId = undefined
    mocks.pathname = '/'
    mocks.commandClient = commandClient()
    mocks.daemonClient = daemonClient()
    uiStore.getState().setSidebarSectionExpanded('pinned', true)
    uiStore.getState().setSidebarSectionExpanded('projects', true)
    uiStore.getState().setSidebarSectionExpanded('conversations', true)
    uiStore.getState().setProjectExpanded('/repo/alpha', true)
  })

  it('lists daemon task projections and navigates by task ID', async () => {
    const listTasks = vi.fn().mockResolvedValue({
      tasks: [taskProjection({ root: defaultRoot })],
      type: 'task_list',
    })
    mocks.daemonClient = daemonClient({ listTasks })

    renderSidebar()

    fireEvent.click(await screen.findByRole('button', { name: 'Daemon conversation' }))
    expect(listTasks).toHaveBeenCalledOnce()
    expect(mocks.navigate).toHaveBeenCalledWith({ search: { taskId }, to: '/' })
  })

  it('creates the primary conversation in the default workspace', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    mocks.daemonClient = daemonClient({ request })

    renderSidebar()
    const newConversation = await screen.findByRole('button', { name: 'New conversation' })
    await waitFor(() => expect(newConversation).toBeEnabled())
    fireEvent.click(newConversation)

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith({
        metadata: expect.objectContaining({ expectedStreamVersion: 0 }),
        title: 'New conversation',
        type: 'create_task',
        workspace: { mode: 'current', root: defaultRoot },
      }),
    )
    expect(mocks.navigate).toHaveBeenCalledWith({ search: { taskId }, to: '/' })
  })

  it('opens and marks the scheduled task page from the fixed sidebar action', async () => {
    mocks.pathname = '/scheduled-tasks'

    renderSidebar()

    const action = await screen.findByRole('button', { name: 'Open scheduled tasks' })
    expect(action).toHaveAttribute('aria-current', 'page')
    fireEvent.click(action)
    expect(mocks.navigate).toHaveBeenCalledWith({ to: '/scheduled-tasks' })
  })

  it('creates a conversation inside its project workspace', async () => {
    const request = vi.fn().mockResolvedValue(acceptedFrame())
    mocks.daemonClient = daemonClient({
      listTasks: vi.fn().mockResolvedValue({
        tasks: [taskProjection({ root: '/repo/alpha' })],
        type: 'task_list',
      }),
      request,
    })

    renderSidebar()
    fireEvent.click(await screen.findByRole('button', { name: 'New conversation in Alpha' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        expect.objectContaining({ workspace: { mode: 'current', root: '/repo/alpha' } }),
      ),
    )
  })

  it('adds a selected project folder and refreshes the project list', async () => {
    const addProject = vi.fn().mockResolvedValue({ project: projects[0] })
    const listProjects = vi.fn().mockResolvedValue({ activePath: null, projects })
    mocks.commandClient = commandClient({ addProject, listProjects })
    mocks.pickProjectDirectory.mockResolvedValue('/repo/new')

    renderSidebar()
    fireEvent.click(await screen.findByRole('button', { name: 'Add project' }))

    await waitFor(() => expect(addProject).toHaveBeenCalledWith('/repo/new'))
    await waitFor(() => expect(listProjects).toHaveBeenCalledTimes(2))
  })

  it('pins a task, refreshes task projections, and removes the active task safely', async () => {
    const user = userEvent.setup()
    const listTasks = vi.fn().mockResolvedValue({
      tasks: [taskProjection({ root: '/repo/alpha' })],
      type: 'task_list',
    })
    const setTaskPinned = vi.fn().mockResolvedValue(acceptedMessage())
    const removeTask = vi.fn().mockResolvedValue(acceptedMessage())
    mocks.daemonClient = daemonClient({ listTasks, removeTask, setTaskPinned })
    mocks.selectedTaskId = taskId

    renderSidebar()
    await user.click(await screen.findByRole('button', { name: 'Daemon conversation actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Pin' }))
    expect(setTaskPinned).toHaveBeenCalledWith(taskId, 1, true)
    await waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))

    await user.click(screen.getByRole('button', { name: 'Daemon conversation actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Remove' }))
    await user.click(screen.getByRole('button', { name: 'Remove conversation' }))
    expect(removeTask).toHaveBeenCalledWith(taskId, 1)
    await waitFor(() => expect(mocks.navigate).toHaveBeenCalledWith({ search: {}, to: '/' }))
  })

  it('shows task mutation errors without discarding the current task list', async () => {
    const user = userEvent.setup()
    const listTasks = vi.fn().mockResolvedValue({
      tasks: [taskProjection({ root: '/repo/alpha' })],
      type: 'task_list',
    })
    const setTaskPinned = vi.fn().mockRejectedValue(new Error('stale task version'))
    mocks.daemonClient = daemonClient({ listTasks, setTaskPinned })

    renderSidebar()
    await user.click(await screen.findByRole('button', { name: 'Daemon conversation actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Pin' }))

    expect(await screen.findByRole('status')).toHaveTextContent('stale task version')
    expect(screen.getByRole('button', { name: 'Daemon conversation' })).toBeInTheDocument()
    expect(listTasks).toHaveBeenCalledOnce()
  })

  it('renames, moves, and removes projects through registry commands', async () => {
    const user = userEvent.setup()
    const renameProject = vi.fn().mockResolvedValue({ project: projects[0] })
    const moveProject = vi.fn().mockResolvedValue({ activePath: null, projects })
    const deleteProject = vi
      .fn()
      .mockResolvedValue({ activePath: null, path: '/repo/alpha', status: 'deleted' })
    mocks.commandClient = commandClient({ deleteProject, moveProject, renameProject })

    renderSidebar()
    await user.click(await screen.findByRole('button', { name: 'Alpha actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const input = screen.getByRole('textbox', { name: 'Project name' })
    await user.clear(input)
    await user.type(input, 'Alpha renamed')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(renameProject).toHaveBeenCalledWith('/repo/alpha', 'Alpha renamed')

    await user.click(screen.getByRole('button', { name: 'Alpha actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Move down' }))
    expect(moveProject).toHaveBeenCalledWith('/repo/alpha', 'down')

    await user.click(screen.getByRole('button', { name: 'Alpha actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Remove project' }))
    await user.click(screen.getByRole('button', { name: 'Remove project' }))
    expect(deleteProject).toHaveBeenCalledWith('/repo/alpha')
  })

  it('refreshes task projections when the daemon publishes events', async () => {
    let onFrame: Parameters<DaemonClient['subscribe']>[1] | undefined
    const subscribe = vi.fn(async (_offset, handler: Parameters<DaemonClient['subscribe']>[1]) => {
      onFrame = handler
      return async () => undefined
    })
    const listTasks = vi.fn().mockResolvedValue({
      tasks: [taskProjection({ root: defaultRoot })],
      type: 'task_list',
    })
    mocks.daemonClient = daemonClient({ listTasks, subscribe })

    renderSidebar()
    await screen.findByRole('button', { name: 'Daemon conversation' })
    await waitFor(() =>
      expect(subscribe).toHaveBeenCalledWith(1, expect.any(Function), expect.any(Function)),
    )

    onFrame?.({
      message: { afterOffset: 1, events: [], gap: false, latestOffset: 2, type: 'event_batch' },
      protocolVersion: 6,
    })

    await waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))
  })
})

function renderSidebar() {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SidebarNav />
    </QueryClientProvider>,
  )
}

function daemonClient(overrides: Partial<DaemonClient> = {}): DaemonClient {
  return {
    connect: vi.fn().mockResolvedValue({}),
    listReferenceCandidates: vi.fn(),
    listTasks: vi.fn().mockResolvedValue({ tasks: [], type: 'task_list' }),
    loadTask: vi.fn(),
    readBlob: vi.fn(),
    removeTask: vi.fn(),
    renameTask: vi.fn(),
    request: vi.fn(),
    setTaskArchived: vi.fn(),
    setTaskPinned: vi.fn(),
    stageBlobFromPath: vi.fn(),
    subscribe: vi.fn().mockResolvedValue(async () => undefined),
    ...overrides,
  } as DaemonClient
}

function commandClient(overrides: Partial<CommandClient> = {}): CommandClient {
  return {
    addProject: vi.fn(),
    deleteProject: vi.fn(),
    getDefaultWorkspace: vi.fn().mockResolvedValue({ path: defaultRoot }),
    listProjects: vi.fn().mockResolvedValue({ activePath: null, projects }),
    moveProject: vi.fn(),
    renameProject: vi.fn(),
    ...overrides,
  } as CommandClient
}

const taskId = '01J00000000000000000000001'
const defaultRoot = '/home/me/.jyowo/workspaces/default'
const projects = [
  { lastOpenedAt: '2026-07-12T00:00:00Z', name: 'Alpha', path: '/repo/alpha' },
  { lastOpenedAt: '2026-07-11T00:00:00Z', name: 'Beta', path: '/repo/beta' },
]

function taskProjection({ root }: { root: string }) {
  return {
    archived: false,
    lastGlobalOffset: 1,
    pinned: false,
    queue: [],
    removed: false,
    state: 'completed' as const,
    streamVersion: 1,
    taskId,
    title: 'Daemon conversation',
    workspace: { mode: 'current' as const, root },
  }
}

function acceptedMessage() {
  return {
    commandId: taskId,
    committedOffset: 1,
    streamVersion: 2,
    taskId,
    type: 'command_accepted' as const,
  }
}

function acceptedFrame() {
  return { message: acceptedMessage(), protocolVersion: 6 }
}
