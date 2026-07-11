import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { DaemonClient } from '@/shared/daemon/client'

import { SidebarNav } from './SidebarNav'

const mocks = vi.hoisted(() => ({
  daemonClient: null as unknown as DaemonClient,
  navigate: vi.fn(),
  selectedTaskId: undefined as string | undefined,
}))

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => mocks.navigate,
  useRouterState: ({ select }: { select: (state: unknown) => unknown }) =>
    select({ location: { search: { taskId: mocks.selectedTaskId } } }),
}))

vi.mock('@/shared/tauri/react', () => ({
  useDaemonClient: () => mocks.daemonClient,
}))

vi.mock('./use-active-project-path', () => ({
  useActiveProjectPath: () => ({ data: '/workspace' }),
}))

describe('SidebarNav task navigation', () => {
  beforeEach(() => {
    mocks.navigate.mockReset().mockResolvedValue(undefined)
    mocks.selectedTaskId = undefined
  })

  it('lists daemon task projections and navigates by task ID', async () => {
    const listTasks = vi.fn().mockResolvedValue({ tasks: [taskProjection()], type: 'task_list' })
    mocks.daemonClient = client({ listTasks })

    renderSidebar()

    fireEvent.click(await screen.findByRole('button', { name: /Daemon task/ }))
    expect(listTasks).toHaveBeenCalledOnce()
    expect(mocks.navigate).toHaveBeenCalledWith({ search: { taskId }, to: '/' })
  })

  it('creates a daemon task in the active workspace and navigates to the accepted task', async () => {
    const listTasks = vi.fn().mockResolvedValue({ tasks: [], type: 'task_list' })
    const request = vi.fn().mockResolvedValue({
      message: {
        commandId: taskId,
        committedOffset: 1,
        streamVersion: 1,
        taskId,
        type: 'command_accepted',
      },
      protocolVersion: 1,
    })
    mocks.daemonClient = client({ listTasks, request })

    renderSidebar()
    fireEvent.click(await screen.findByRole('button', { name: 'New task' }))

    await waitFor(() =>
      expect(request).toHaveBeenCalledWith({
        metadata: expect.objectContaining({
          commandId: expect.stringMatching(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/),
          expectedStreamVersion: 0,
          idempotencyKey: expect.any(String),
        }),
        title: 'New task',
        type: 'create_task',
        workspace: { mode: 'current', root: '/workspace' },
      }),
    )
    expect(mocks.navigate).toHaveBeenCalledWith({ search: { taskId }, to: '/' })
  })

  it('navigates after task creation even when refreshing the list fails', async () => {
    const listTasks = vi
      .fn()
      .mockResolvedValueOnce({ tasks: [], type: 'task_list' })
      .mockRejectedValueOnce(new Error('refresh failed'))
    mocks.daemonClient = client({
      listTasks,
      request: vi.fn().mockResolvedValue(acceptedFrame()),
    })

    renderSidebar()
    fireEvent.click(await screen.findByRole('button', { name: 'New task' }))

    await waitFor(() =>
      expect(mocks.navigate).toHaveBeenCalledWith({ search: { taskId }, to: '/' }),
    )
  })

  it('refreshes task projections when the daemon publishes events', async () => {
    let onFrame: Parameters<DaemonClient['subscribe']>[1] | undefined
    const subscribe = vi.fn(async (_offset, handler: Parameters<DaemonClient['subscribe']>[1]) => {
      onFrame = handler
      return async () => undefined
    })
    const listTasks = vi.fn().mockResolvedValue({
      tasks: [taskProjection()],
      type: 'task_list',
    })
    mocks.daemonClient = client({ listTasks, subscribe })

    renderSidebar()
    await screen.findByRole('button', { name: /Daemon task/ })
    await waitFor(() =>
      expect(subscribe).toHaveBeenCalledWith(1, expect.any(Function), expect.any(Function)),
    )

    onFrame?.({
      message: {
        afterOffset: 1,
        events: [],
        gap: false,
        latestOffset: 2,
        type: 'event_batch',
      },
      protocolVersion: 1,
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

function client(overrides: Partial<DaemonClient>): DaemonClient {
  return {
    connect: vi.fn().mockResolvedValue(undefined),
    listTasks: vi.fn(),
    loadTask: vi.fn(),
    readBlob: vi.fn(),
    request: vi.fn(),
    subscribe: vi.fn().mockResolvedValue(async () => undefined),
    ...overrides,
  } as DaemonClient
}

const taskId = '01J00000000000000000000001'

function taskProjection() {
  return {
    archived: false,
    lastGlobalOffset: 1,
    queue: [],
    state: 'completed' as const,
    streamVersion: 1,
    taskId,
    title: 'Daemon task',
  }
}

function acceptedFrame() {
  return {
    message: {
      commandId: taskId,
      committedOffset: 1,
      streamVersion: 1,
      taskId,
      type: 'command_accepted' as const,
    },
    protocolVersion: 1,
  }
}
