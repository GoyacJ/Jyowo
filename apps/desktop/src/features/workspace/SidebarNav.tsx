import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { TaskList } from '@/features/tasks/TaskList'
import { createTaskCreationMetadata } from '@/features/tasks/task-command'
import type { TaskProjection, TypedUlid } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import { pickProjectDirectory } from '@/shared/tauri/file-dialog'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
const TASKS_QUERY_KEY = ['daemon-tasks'] as const
const WORKSPACES_QUERY_KEY = ['sidebar-workspaces'] as const

type SidebarNavProps = {
  compact?: boolean
}

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const { t } = useTranslation('shell')
  const client = useDaemonClient()
  const commandClient = useCommandClient()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const sidebarSections = useUiStore((state) => state.sidebarSections)
  const expandedProjects = useUiStore((state) => state.expandedProjects)
  const setSidebarSectionExpanded = useUiStore((state) => state.setSidebarSectionExpanded)
  const setProjectExpanded = useUiStore((state) => state.setProjectExpanded)
  const activeTaskId = useRouterState({
    select: (state) => state.location.search.taskId,
  }) as TypedUlid | undefined
  const isCompact = compact || sidebarCollapsed

  const workspacesQuery = useQuery({
    queryFn: async () => {
      const [projectList, defaultWorkspace] = await Promise.all([
        commandClient.listProjects(),
        commandClient.getDefaultWorkspace(),
      ])
      return { defaultRoot: defaultWorkspace.path, projects: projectList.projects }
    },
    queryKey: WORKSPACES_QUERY_KEY,
  })

  const tasksQuery = useQuery({
    queryFn: async () => {
      await client.connect()
      return client.listTasks()
    },
    queryKey: TASKS_QUERY_KEY,
  })
  const createTask = useMutation({
    mutationFn: async (root: string) => {
      const frame = await client.request({
        metadata: createTaskCreationMetadata(),
        title: t('actions.newConversation'),
        type: 'create_task',
        workspace: { mode: 'current', root },
      })
      if (frame.message.type === 'command_rejected') {
        throw new Error(frame.message.reason.replaceAll('_', ' '))
      }
      if (frame.message.type === 'error') throw new Error(frame.message.message)
      if (frame.message.type !== 'command_accepted') {
        throw new Error(`Expected command_accepted, received ${frame.message.type}`)
      }
      return frame.message.taskId
    },
    onSuccess: async (taskId) => {
      await navigate({ search: { taskId }, to: '/' })
      void queryClient.invalidateQueries({ queryKey: TASKS_QUERY_KEY })
    },
  })
  const addProject = useMutation({
    mutationFn: async () => {
      const path = await pickProjectDirectory()
      if (!path) return false
      await commandClient.addProject(path)
      return true
    },
    onSuccess: (added) => {
      if (added) void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY })
    },
  })
  const renameProject = useMutation({
    mutationFn: ({ name, path }: { name: string; path: string }) =>
      commandClient.renameProject(path, name),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY }),
  })
  const moveProject = useMutation({
    mutationFn: ({ direction, path }: { direction: 'up' | 'down'; path: string }) =>
      commandClient.moveProject(path, direction),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY }),
  })
  const removeProject = useMutation({
    mutationFn: (path: string) => commandClient.deleteProject(path),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY }),
  })

  async function refreshTasks() {
    await queryClient.invalidateQueries({ queryKey: TASKS_QUERY_KEY })
  }

  async function setTaskPinned(task: TaskProjection, pinned: boolean) {
    await client.setTaskPinned(task.taskId, task.streamVersion, pinned)
    await refreshTasks()
  }

  async function renameTask(task: TaskProjection, title: string) {
    await client.renameTask(task.taskId, task.streamVersion, title)
    await refreshTasks()
  }

  async function setTaskArchived(task: TaskProjection, archived: boolean) {
    await client.setTaskArchived(task.taskId, task.streamVersion, archived)
    await refreshTasks()
    if (task.taskId === activeTaskId) await navigate({ search: {}, to: '/' })
  }

  async function removeTask(task: TaskProjection) {
    await client.removeTask(task.taskId, task.streamVersion)
    await refreshTasks()
    if (task.taskId === activeTaskId) await navigate({ search: {}, to: '/' })
  }

  useEffect(() => {
    if (!tasksQuery.data) return
    let cancelled = false
    let unsubscribe: (() => Promise<void>) | undefined
    const afterOffset = tasksQuery.data.tasks.reduce(
      (offset, task) => Math.max(offset, task.lastGlobalOffset),
      0,
    )
    void client
      .subscribe(
        afterOffset,
        () => void queryClient.invalidateQueries({ queryKey: TASKS_QUERY_KEY }),
        () => void queryClient.invalidateQueries({ queryKey: TASKS_QUERY_KEY }),
      )
      .then((close) => {
        if (cancelled) {
          void close?.()
          return
        }
        unsubscribe = close
      })
      .catch(() => undefined)

    return () => {
      cancelled = true
      void unsubscribe?.()
    }
  }, [client, queryClient, tasksQuery.data])

  function selectTask(taskId: TypedUlid) {
    void navigate({ search: { taskId }, to: '/' })
  }

  function runPaletteAction(action: CommandPaletteAction) {
    if (action === 'new-conversation') {
      const root = workspacesQuery.data?.defaultRoot
      if (root) createTask.mutate(root)
      return
    }
    void navigate({ to: action === 'settings' ? '/settings' : '/evals' })
  }

  const error =
    createTask.error ??
    addProject.error ??
    renameProject.error ??
    moveProject.error ??
    removeProject.error ??
    tasksQuery.error ??
    workspacesQuery.error

  return (
    <aside
      aria-label="Workspace"
      className="flex min-h-0 flex-col border-border border-r bg-raised-surface"
      data-collapsed={isCompact}
    >
      <div
        className={cn('flex h-12 items-center', isCompact ? 'justify-center' : 'justify-end px-2')}
      >
        <Button
          aria-label={
            sidebarCollapsed ? t('actions.expandSidebar') : t('actions.collapseSidebar')
          }
          className="size-8"
          onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
          size="icon"
          type="button"
          variant="ghost"
        >
          {sidebarCollapsed ? (
            <PanelLeftOpen aria-hidden="true" className="size-4" />
          ) : (
            <PanelLeftClose aria-hidden="true" className="size-4" />
          )}
        </Button>
      </div>

      <TaskList
        activeTaskId={activeTaskId}
        compact={isCompact}
        creating={createTask.isPending}
        defaultRoot={workspacesQuery.data?.defaultRoot ?? ''}
        expandedProjects={expandedProjects}
        loading={tasksQuery.isLoading || workspacesQuery.isLoading}
        onAddProject={() => addProject.mutate()}
        onCreateConversation={(root) => createTask.mutate(root)}
        onMoveProject={(path, direction) => moveProject.mutate({ direction, path })}
        onRemoveProject={(path) => removeProject.mutate(path)}
        onRemoveTask={removeTask}
        onRenameProject={(path, name) => renameProject.mutate({ name, path })}
        onRenameTask={renameTask}
        onSelectTask={selectTask}
        onSetTaskArchived={setTaskArchived}
        onSetTaskPinned={setTaskPinned}
        onToggleProject={setProjectExpanded}
        onToggleSection={setSidebarSectionExpanded}
        projects={workspacesQuery.data?.projects ?? []}
        sections={sidebarSections}
        tasks={tasksQuery.data?.tasks ?? []}
      />

      {error && !isCompact ? (
        <p aria-live="polite" className="px-4 pb-3 text-destructive text-xs" role="status">
          {error instanceof Error ? error.message : String(error)}
        </p>
      ) : null}
      <CommandPalette onAction={runPaletteAction} />
    </aside>
  )
}
