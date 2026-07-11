import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { useEffect } from 'react'

import { TaskList } from '@/features/tasks/TaskList'
import { createTaskCreationMetadata } from '@/features/tasks/task-command'
import type { TypedUlid } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import { useDaemonClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { useActiveProjectPath } from './use-active-project-path'

const TASKS_QUERY_KEY = ['daemon-tasks'] as const

type SidebarNavProps = {
  compact?: boolean
}

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const client = useDaemonClient()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const workspacePath = useActiveProjectPath().data
  const activeTaskId = useRouterState({
    select: (state) => state.location.search.taskId,
  }) as TypedUlid | undefined
  const isCompact = compact || sidebarCollapsed

  const tasksQuery = useQuery({
    queryFn: async () => {
      await client.connect()
      return client.listTasks()
    },
    queryKey: TASKS_QUERY_KEY,
  })
  const createTask = useMutation({
    mutationFn: async () => {
      if (!workspacePath) throw new Error('No active workspace')
      const frame = await client.request({
        metadata: createTaskCreationMetadata(),
        title: 'New task',
        type: 'create_task',
        workspace: { mode: 'current', root: workspacePath },
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
      createTask.mutate()
      return
    }
    void navigate({ to: action === 'settings' ? '/settings' : '/evals' })
  }

  const error = createTask.error ?? tasksQuery.error

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
          aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
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
        onCreateTask={() => createTask.mutate()}
        onSelectTask={selectTask}
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
