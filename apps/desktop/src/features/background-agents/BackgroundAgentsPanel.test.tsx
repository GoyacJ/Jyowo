import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { TaskProjection, TaskState } from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'
import { AppI18nProvider } from '@/shared/i18n/i18n'
import { DaemonClientProvider } from '@/shared/tauri/react'

import { BackgroundAgentsPanel } from './BackgroundAgentsPanel'

function task(index: number, state: TaskState, attachment: 'attached' | 'detached') {
  return {
    archived: false,
    lastGlobalOffset: index,
    parent: {
      attachment,
      delegationId: id(index + 30),
      parentSegmentId: id(index + 20),
      parentTaskId: id(index + 10),
    },
    queue: [],
    state,
    streamVersion: 2,
    taskId: id(index),
    title: `Background job ${index}`,
  } satisfies TaskProjection
}

function id(index: number) {
  return `01J00000000000000000000${String(index).padStart(2, '0')}`
}

function daemonClient(tasks: TaskProjection[] = []) {
  return {
    listTasks: vi.fn(async () => ({ tasks, type: 'task_list' as const })),
    removeTask: vi.fn(),
    request: vi.fn(async () => ({
      message: {
        commandId: id(80),
        committedOffset: 3,
        streamVersion: 3,
        taskId: id(1),
        type: 'command_accepted' as const,
      },
      protocolVersion: 6,
    })),
    setTaskArchived: vi.fn(),
  } as unknown as DaemonClient
}

function renderPanel(client: DaemonClient, selectedBackgroundAgentId?: string) {
  const queryClient = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  })
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <DaemonClientProvider client={client}>
        <QueryClientProvider client={queryClient}>
          <AppI18nProvider>{children}</AppI18nProvider>
        </QueryClientProvider>
      </DaemonClientProvider>
    )
  }
  return render(<BackgroundAgentsPanel selectedBackgroundAgentId={selectedBackgroundAgentId} />, {
    wrapper: Wrapper,
  })
}

describe('BackgroundAgentsPanel', () => {
  it('shows only detached child task projections', async () => {
    renderPanel(daemonClient([task(1, 'running', 'detached'), task(2, 'running', 'attached')]))

    expect(await screen.findByText('Background job 1')).toBeInTheDocument()
    expect(screen.queryByText('Background job 2')).not.toBeInTheDocument()
  })

  it('maps lifecycle and input actions to daemon task requests', async () => {
    const client = daemonClient([
      task(1, 'running', 'detached'),
      task(2, 'interrupted', 'detached'),
      task(3, 'completed', 'detached'),
      { ...task(4, 'completed', 'detached'), archived: true },
    ])
    renderPanel(client)

    const running = await screen.findByRole('article', { name: 'Background job 1' })
    fireEvent.click(within(running).getByRole('button', { name: '暂停' }))
    fireEvent.click(within(running).getByRole('button', { name: '取消' }))

    const interrupted = screen.getByRole('article', { name: 'Background job 2' })
    fireEvent.click(within(interrupted).getByRole('button', { name: '恢复' }))
    fireEvent.change(within(interrupted).getByLabelText('输入'), {
      target: { value: 'Continue safely' },
    })
    fireEvent.click(within(interrupted).getByRole('button', { name: '发送输入' }))

    fireEvent.click(
      within(screen.getByRole('article', { name: 'Background job 3' })).getByRole('button', {
        name: '归档',
      }),
    )
    fireEvent.click(
      within(screen.getByRole('article', { name: 'Background job 4' })).getByRole('button', {
        name: '删除',
      }),
    )

    await waitFor(() => {
      expect(client.request).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'safe_point', taskId: id(1), type: 'stop_run' }),
      )
      expect(client.request).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'force', taskId: id(1), type: 'stop_run' }),
      )
      expect(client.request).toHaveBeenCalledWith(
        expect.objectContaining({ taskId: id(2), type: 'continue_task' }),
      )
      expect(client.request).toHaveBeenCalledWith(
        expect.objectContaining({
          content: 'Continue safely',
          taskId: id(2),
          type: 'submit_message',
        }),
      )
      expect(client.setTaskArchived).toHaveBeenCalledWith(id(3), 2, true)
      expect(client.removeTask).toHaveBeenCalledWith(id(4), 2)
    })
  })
})
