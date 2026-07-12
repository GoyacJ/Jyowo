import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { TaskProjection, TaskState } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import type { ListProjectsResponse } from '@/shared/tauri/commands'

import { groupSidebarTasks, TaskList } from './TaskList'

const defaultRoot = '/home/me/.jyowo/workspaces/default'
const projects: ListProjectsResponse['projects'] = [
  { lastOpenedAt: '2026-07-12T00:00:00Z', name: 'Alpha', path: '/repo/alpha' },
  { lastOpenedAt: '2026-07-11T00:00:00Z', name: 'Beta', path: '/repo/beta' },
]

describe('groupSidebarTasks', () => {
  it('places pinned tasks only in pinned and groups remaining tasks by canonical root', () => {
    const groups = groupSidebarTasks(
      [
        projection(1, 'completed', { pinned: true, root: '/repo/alpha' }),
        projection(2, 'completed', { root: '/repo/alpha' }),
        projection(3, 'completed', { root: defaultRoot }),
        projection(4, 'completed', { root: '/unknown/root' }),
      ],
      projects,
      defaultRoot,
    )

    expect(groups.pinned.map((task) => task.taskId)).toEqual([taskId(1)])
    expect(groups.projects[0]?.tasks.map((task) => task.taskId)).toEqual([taskId(2)])
    expect(groups.projects[1]?.tasks).toEqual([])
    expect(groups.conversations.map((task) => task.taskId)).toEqual([taskId(4), taskId(3)])
  })
})

describe('TaskList', () => {
  it('renders three independently collapsible sections and project rows', () => {
    const onToggleSection = vi.fn()
    const onToggleProject = vi.fn()
    renderTaskList({ onToggleProject, onToggleSection })

    const pinned = screen.getByRole('button', { name: 'Pinned' })
    const projectSection = screen.getByRole('button', { name: 'Projects' })
    const conversations = screen.getByRole('button', { name: 'Conversations' })
    expect(pinned).toHaveAttribute('aria-expanded', 'true')
    expect(projectSection).toHaveAttribute('aria-expanded', 'true')
    expect(conversations).toHaveAttribute('aria-expanded', 'false')

    fireEvent.click(pinned)
    fireEvent.click(projectSection)
    fireEvent.click(conversations)
    expect(onToggleSection).toHaveBeenNthCalledWith(1, 'pinned', false)
    expect(onToggleSection).toHaveBeenNthCalledWith(2, 'projects', false)
    expect(onToggleSection).toHaveBeenNthCalledWith(3, 'conversations', true)

    const alpha = screen.getByRole('button', { name: 'Alpha' })
    expect(alpha).toHaveAttribute('aria-expanded', 'true')
    fireEvent.click(alpha)
    expect(onToggleProject).toHaveBeenCalledWith('/repo/alpha', false)
  })

  it('keeps pinned tasks out of project and conversation lists', () => {
    renderTaskList()

    const pinned = screen.getByRole('region', { name: 'Pinned' })
    expect(within(pinned).getByText('Task 1')).toBeInTheDocument()
    const alpha = screen.getByRole('region', { name: 'Alpha conversations' })
    expect(within(alpha).getByText('Task 2')).toBeInTheDocument()
    expect(within(alpha).queryByText('Task 1')).not.toBeInTheDocument()
  })

  it('creates conversations globally and inside a project', () => {
    const onCreateConversation = vi.fn()
    renderTaskList({ onCreateConversation })

    fireEvent.click(screen.getByRole('button', { name: 'New conversation' }))
    expect(onCreateConversation).toHaveBeenCalledWith(defaultRoot)
    fireEvent.click(screen.getByRole('button', { name: 'New conversation in Alpha' }))
    expect(onCreateConversation).toHaveBeenCalledWith('/repo/alpha')
  })

  it('exposes project addition from the projects section', () => {
    const onAddProject = vi.fn()
    renderTaskList({ onAddProject })

    fireEvent.click(screen.getByRole('button', { name: 'Add project' }))
    expect(onAddProject).toHaveBeenCalledOnce()
  })

  it('invokes task menu actions including validated rename and confirmation actions', async () => {
    const user = userEvent.setup()
    const onSetTaskPinned = vi.fn()
    const onRenameTask = vi.fn()
    const onSetTaskArchived = vi.fn()
    const onRemoveTask = vi.fn()
    renderTaskList({ onRemoveTask, onRenameTask, onSetTaskArchived, onSetTaskPinned })

    await user.click(screen.getByRole('button', { name: 'Task 2 actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Pin' }))
    expect(onSetTaskPinned).toHaveBeenCalledWith(
      expect.objectContaining({ taskId: taskId(2) }),
      true,
    )

    await user.click(screen.getByRole('button', { name: 'Task 2 actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const taskName = screen.getByRole('textbox', { name: 'Conversation name' })
    await user.clear(taskName)
    await user.type(taskName, 'Renamed conversation')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(onRenameTask).toHaveBeenCalledWith(
      expect.objectContaining({ taskId: taskId(2) }),
      'Renamed conversation',
    )

    await user.click(screen.getByRole('button', { name: 'Task 2 actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Archive' }))
    await user.click(screen.getByRole('button', { name: 'Archive conversation' }))
    expect(onSetTaskArchived).toHaveBeenCalledWith(
      expect.objectContaining({ taskId: taskId(2) }),
      true,
    )

    await user.click(screen.getByRole('button', { name: 'Task 2 actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Remove' }))
    await user.click(screen.getByRole('button', { name: 'Remove conversation' }))
    expect(onRemoveTask).toHaveBeenCalledWith(expect.objectContaining({ taskId: taskId(2) }))
  })

  it('invokes project menu actions and disables unavailable moves', async () => {
    const user = userEvent.setup()
    const onRenameProject = vi.fn()
    const onMoveProject = vi.fn()
    const onRemoveProject = vi.fn()
    renderTaskList({ onMoveProject, onRemoveProject, onRenameProject })

    await user.click(screen.getByRole('button', { name: 'Alpha actions' }))
    expect(screen.getByRole('menuitem', { name: 'Move up' })).toHaveAttribute('data-disabled')
    await user.click(screen.getByRole('menuitem', { name: 'Move down' }))
    expect(onMoveProject).toHaveBeenCalledWith('/repo/alpha', 'down')

    await user.click(screen.getByRole('button', { name: 'Alpha actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const projectName = screen.getByRole('textbox', { name: 'Project name' })
    await user.clear(projectName)
    await user.type(projectName, 'Alpha renamed')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(onRenameProject).toHaveBeenCalledWith('/repo/alpha', 'Alpha renamed')

    await user.click(screen.getByRole('button', { name: 'Alpha actions' }))
    await user.click(screen.getByRole('menuitem', { name: 'Remove project' }))
    await user.click(screen.getByRole('button', { name: 'Remove project' }))
    expect(onRemoveProject).toHaveBeenCalledWith('/repo/alpha')
  })

  it('keeps loading and empty sections navigable', () => {
    renderTaskList({ loading: true, projects: [], tasks: [] })
    expect(screen.getByRole('button', { name: 'New conversation' })).toBeEnabled()
    expect(screen.getByRole('status')).toHaveTextContent('Loading conversations')
    expect(screen.getByRole('button', { name: 'Pinned' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Projects' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Conversations' })).toBeInTheDocument()
  })

  it('localizes task status labels in Chinese', () => {
    renderTaskList({ sections: { conversations: true, pinned: true, projects: true } }, 'zh-CN')

    expect(screen.getByText('Task 2').closest('button')).toHaveAttribute('title', '已完成')
    expect(screen.getByText('Task 3').closest('button')).toHaveAttribute('title', '就绪')
  })
})

type TaskListOverrides = Partial<React.ComponentProps<typeof TaskList>>

function renderTaskList(overrides: TaskListOverrides = {}, locale: 'en-US' | 'zh-CN' = 'en-US') {
  const tasks = [
    projection(1, 'completed', { pinned: true, root: '/repo/alpha' }),
    projection(2, 'completed', { root: '/repo/alpha' }),
    projection(3, 'idle', { root: defaultRoot }),
  ]
  return render(
    <I18nextProvider i18n={createAppI18n(locale)}>
      <TaskList
        activeTaskId={taskId(2)}
        defaultRoot={defaultRoot}
        expandedProjects={{ '/repo/alpha': true, '/repo/beta': false }}
        onAddProject={vi.fn()}
        onCreateConversation={vi.fn()}
        onMoveProject={vi.fn()}
        onRemoveProject={vi.fn()}
        onRemoveTask={vi.fn()}
        onRenameProject={vi.fn()}
        onRenameTask={vi.fn()}
        onSelectTask={vi.fn()}
        onSetTaskArchived={vi.fn()}
        onSetTaskPinned={vi.fn()}
        onToggleProject={vi.fn()}
        onToggleSection={vi.fn()}
        projects={projects}
        sections={{ conversations: false, pinned: true, projects: true }}
        tasks={tasks}
        {...overrides}
      />
    </I18nextProvider>,
  )
}

function projection(
  index: number,
  state: TaskState,
  options: { archived?: boolean; pinned?: boolean; root?: string } = {},
): TaskProjection {
  return {
    archived: options.archived ?? false,
    lastGlobalOffset: index,
    pinned: options.pinned ?? false,
    queue: [],
    removed: false,
    state,
    streamVersion: index,
    taskId: taskId(index),
    title: `Task ${index}`,
    workspace: options.root ? { mode: 'current', root: options.root } : null,
  }
}

function taskId(index: number) {
  return `01J00000000000000000000${String(index).padStart(2, '0')}`
}
