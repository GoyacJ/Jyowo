import {
  Archive,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CirclePause,
  LoaderCircle,
  MessageSquareMore,
  Plus,
} from 'lucide-react'

import type { TaskProjection, TypedUlid } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'

export function TaskList({
  activeTaskId,
  compact = false,
  creating = false,
  onCreateTask,
  onSelectTask,
  tasks,
}: {
  activeTaskId?: TypedUlid
  compact?: boolean
  creating?: boolean
  onCreateTask: () => void
  onSelectTask: (taskId: TypedUlid) => void
  tasks: TaskProjection[]
}) {
  const groups = groupTasks(tasks)

  return (
    <nav aria-label="Tasks" className="flex min-h-0 flex-1 flex-col">
      <div className="px-2 pb-2">
        <button
          aria-label="New task"
          className={cn(
            'flex h-9 w-full items-center rounded-md border border-border bg-surface-raised text-sm hover:bg-row-muted disabled:opacity-50',
            compact ? 'justify-center px-0' : 'gap-2 px-2.5',
          )}
          disabled={creating}
          onClick={onCreateTask}
          type="button"
        >
          {creating ? (
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          ) : (
            <Plus aria-hidden="true" className="size-4" />
          )}
          {compact ? null : <span>{creating ? 'Creating…' : 'New task'}</span>}
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
        <TaskGroup
          activeTaskId={activeTaskId}
          compact={compact}
          label="Active tasks"
          onSelectTask={onSelectTask}
          tasks={groups.active}
        />
        <TaskGroup
          activeTaskId={activeTaskId}
          compact={compact}
          label="Recent tasks"
          onSelectTask={onSelectTask}
          tasks={groups.recent}
        />
        <TaskGroup
          activeTaskId={activeTaskId}
          compact={compact}
          label="Archived tasks"
          onSelectTask={onSelectTask}
          tasks={groups.archived}
        />
      </div>
    </nav>
  )
}

function TaskGroup({
  activeTaskId,
  compact,
  label,
  onSelectTask,
  tasks,
}: {
  activeTaskId?: TypedUlid
  compact: boolean
  label: string
  onSelectTask: (taskId: TypedUlid) => void
  tasks: TaskProjection[]
}) {
  if (tasks.length === 0) return null
  return (
    <section aria-label={label} className="mb-4">
      {compact ? null : (
        <h2 className="px-2 pb-1.5 font-medium text-[11px] text-muted-foreground uppercase tracking-[0.08em]">
          {label.replace(' tasks', '')}
        </h2>
      )}
      <ul className="space-y-0.5">
        {tasks.map((task) => (
          <li key={task.taskId}>
            <button
              aria-current={task.taskId === activeTaskId ? 'page' : undefined}
              aria-label={compact ? `${task.title}, ${taskStatus(task).label}` : undefined}
              className={cn(
                'flex w-full items-center rounded-md text-left hover:bg-muted',
                compact ? 'h-9 justify-center' : 'gap-2.5 px-2 py-2',
                task.taskId === activeTaskId && 'bg-selection text-foreground',
              )}
              onClick={() => onSelectTask(task.taskId)}
              title={compact ? task.title : undefined}
              type="button"
            >
              <TaskStatusIcon status={taskStatus(task).key} />
              {compact ? null : (
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] text-foreground">{task.title}</span>
                  <span className="mt-0.5 block text-[11px] text-muted-foreground">
                    {taskStatus(task).label}
                  </span>
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

type DisplayStatus =
  | 'archived'
  | 'completed'
  | 'failed'
  | 'idle'
  | 'interrupted'
  | 'queued'
  | 'running'
  | 'waiting'

function taskStatus(task: TaskProjection): { key: DisplayStatus; label: string } {
  if (task.archived) return { key: 'archived', label: 'Archived' }
  if (task.state === 'running' || task.state === 'yielding') {
    return { key: 'running', label: task.state === 'yielding' ? 'Yielding' : 'Running' }
  }
  if (task.state === 'waiting_permission') return { key: 'waiting', label: 'Waiting permission' }
  if (task.queue.some((item) => item.state === 'queued' || item.state === 'promoting')) {
    const count = task.queue.filter(
      (item) => item.state === 'queued' || item.state === 'promoting',
    ).length
    return { key: 'queued', label: `${count} queued` }
  }
  if (task.state === 'interrupted') return { key: 'interrupted', label: 'Interrupted' }
  if (task.state === 'failed') return { key: 'failed', label: 'Failed' }
  return {
    key: task.state === 'idle' ? 'idle' : 'completed',
    label: task.state === 'idle' ? 'Ready' : 'Completed',
  }
}

function TaskStatusIcon({ status }: { status: DisplayStatus }) {
  const className = cn('size-4 shrink-0', statusColor(status))
  if (status === 'running')
    return <LoaderCircle aria-hidden="true" className={`${className} animate-spin`} />
  if (status === 'waiting') return <CirclePause aria-hidden="true" className={className} />
  if (status === 'queued') return <MessageSquareMore aria-hidden="true" className={className} />
  if (status === 'interrupted') return <CircleDashed aria-hidden="true" className={className} />
  if (status === 'failed') return <CircleAlert aria-hidden="true" className={className} />
  if (status === 'archived') return <Archive aria-hidden="true" className={className} />
  return <CircleCheck aria-hidden="true" className={className} />
}

function statusColor(status: DisplayStatus) {
  if (status === 'running') return 'text-state-running'
  if (status === 'waiting') return 'text-state-waiting'
  if (status === 'queued') return 'text-state-queued'
  if (status === 'interrupted') return 'text-state-interrupted'
  if (status === 'failed') return 'text-state-failed'
  if (status === 'archived') return 'text-state-archived'
  if (status === 'idle') return 'text-state-idle'
  return 'text-state-completed'
}

function groupTasks(tasks: TaskProjection[]) {
  const ordered = [...tasks].sort(
    (left, right) =>
      right.lastGlobalOffset - left.lastGlobalOffset || left.title.localeCompare(right.title),
  )
  return {
    active: ordered.filter(
      (task) =>
        (!task.archived && ['running', 'waiting_permission', 'yielding'].includes(task.state)) ||
        (!task.archived &&
          task.queue.some((item) => item.state === 'queued' || item.state === 'promoting')),
    ),
    archived: ordered.filter((task) => task.archived),
    recent: ordered.filter(
      (task) =>
        !task.archived &&
        !['running', 'waiting_permission', 'yielding'].includes(task.state) &&
        !task.queue.some((item) => item.state === 'queued' || item.state === 'promoting'),
    ),
  }
}
