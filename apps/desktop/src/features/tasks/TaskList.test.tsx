import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import type { TaskProjection, TaskState } from '@/generated/daemon-protocol'

import { TaskList } from './TaskList'

describe('TaskList', () => {
  it('shows daemon task states with text and groups archived tasks separately', () => {
    render(
      <TaskList
        activeTaskId={taskId(1)}
        onCreateTask={vi.fn()}
        onSelectTask={vi.fn()}
        tasks={[
          projection(1, 'running'),
          projection(2, 'idle', { queueCount: 2 }),
          projection(3, 'waiting_permission'),
          projection(4, 'interrupted'),
          projection(5, 'failed'),
          projection(6, 'completed'),
          projection(7, 'completed', { archived: true }),
          projection(8, 'running', { archived: true }),
        ]}
      />,
    )

    const active = screen.getByRole('region', { name: 'Active tasks' })
    expect(within(active).getByText('Running')).toBeInTheDocument()
    expect(within(active).getByText('2 queued')).toBeInTheDocument()
    expect(within(active).getByText('Waiting permission')).toBeInTheDocument()
    const recent = screen.getByRole('region', { name: 'Recent tasks' })
    expect(within(recent).getByText('Interrupted')).toBeInTheDocument()
    expect(within(recent).getByText('Failed')).toBeInTheDocument()
    expect(within(recent).getByText('Completed')).toBeInTheDocument()
    const archived = screen.getByRole('region', { name: 'Archived tasks' })
    expect(archived).toHaveTextContent('Task 7')
    expect(archived).toHaveTextContent('Task 8')
    expect(active).not.toHaveTextContent('Task 8')
  })

  it('selects a task and exposes task creation', () => {
    const onCreateTask = vi.fn()
    const onSelectTask = vi.fn()
    render(
      <TaskList
        onCreateTask={onCreateTask}
        onSelectTask={onSelectTask}
        tasks={[projection(1, 'completed')]}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: /Task 1/ }))
    expect(onSelectTask).toHaveBeenCalledWith(taskId(1))
    fireEvent.click(screen.getByRole('button', { name: 'New task' }))
    expect(onCreateTask).toHaveBeenCalledOnce()
  })

  it('reaches task creation and task selection from the keyboard', async () => {
    const user = userEvent.setup()
    const onCreateTask = vi.fn()
    const onSelectTask = vi.fn()
    render(
      <TaskList
        onCreateTask={onCreateTask}
        onSelectTask={onSelectTask}
        tasks={[projection(1, 'completed')]}
      />,
    )

    await user.tab()
    expect(screen.getByRole('button', { name: 'New task' })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(onCreateTask).toHaveBeenCalledOnce()

    await user.tab()
    expect(screen.getByRole('button', { name: /Task 1/ })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(onSelectTask).toHaveBeenCalledWith(taskId(1))
  })

  it('uses the idle state token for a ready task', () => {
    render(
      <TaskList onCreateTask={vi.fn()} onSelectTask={vi.fn()} tasks={[projection(1, 'idle')]} />,
    )

    const task = screen.getByRole('button', { name: /Task 1/ })
    expect(task.querySelector('svg')).toHaveClass('text-state-idle')
  })
})

function projection(
  index: number,
  state: TaskState,
  options: { archived?: boolean; queueCount?: number } = {},
): TaskProjection {
  return {
    archived: options.archived ?? false,
    lastGlobalOffset: index,
    queue: Array.from({ length: options.queueCount ?? 0 }, (_, queueIndex) => ({
      attachments: [],
      content: `Queue ${queueIndex}`,
      contextReferences: [],
      createdAt: '2026-07-11T00:00:00Z',
      createdGlobalOffset: index + queueIndex,
      queueItemId: taskId(20 + queueIndex),
      revision: 1,
      state: 'queued' as const,
    })),
    state,
    streamVersion: index,
    taskId: taskId(index),
    title: `Task ${index}`,
  }
}

function taskId(index: number) {
  return `01J00000000000000000000${String(index).padStart(2, '0')}`
}
