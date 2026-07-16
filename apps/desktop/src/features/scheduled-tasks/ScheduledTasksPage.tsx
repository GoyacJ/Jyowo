import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import {
  CalendarClock,
  ChevronDown,
  ChevronUp,
  CircleAlert,
  Clock3,
  MoreHorizontal,
  Pencil,
  Play,
  Plus,
  Search,
  Trash2,
} from 'lucide-react'
import { type FormEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type {
  PermissionMode,
  ScheduledTaskRunRecord,
  ScheduledTaskRunStatus,
  ScheduledTaskSpec,
  TypedUlid,
} from '@/generated/daemon-protocol'
import { hasObviousUnredactedSecret } from '@/shared/tauri/commands'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shared/ui/dropdown-menu'
import { EmptyState } from '@/shared/ui/empty-state'
import { FieldControl } from '@/shared/ui/field'
import { Input } from '@/shared/ui/input'
import { Select } from '@/shared/ui/select'
import { StatusBadge, type StatusBadgeProps } from '@/shared/ui/status-badge'
import { Textarea } from '@/shared/ui/textarea'

const TASKS_QUERY_KEY = ['scheduled-tasks'] as const
const RUNS_QUERY_KEY = ['scheduled-task-runs'] as const
const WORKSPACES_QUERY_KEY = ['scheduled-task-workspaces'] as const

type WorkspaceOption = { label: string; value: string }
type StatusFilter = 'all' | 'enabled' | 'paused'

export function ScheduledTasksPage() {
  const { t } = useTranslation('scheduledTasks')
  const daemonClient = useDaemonClient()
  const commandClient = useCommandClient()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all')
  const [workspaceFilter, setWorkspaceFilter] = useState('all')
  const [expandedTaskId, setExpandedTaskId] = useState<string>()
  const [formOpen, setFormOpen] = useState(false)
  const [editingTask, setEditingTask] = useState<ScheduledTaskSpec>()
  const [deletingTask, setDeletingTask] = useState<ScheduledTaskSpec>()

  const tasksQuery = useQuery({
    queryFn: async () => {
      await daemonClient.connect()
      return daemonClient.listScheduledTasks()
    },
    queryKey: TASKS_QUERY_KEY,
  })
  const runsQuery = useQuery({
    queryFn: async () => {
      await daemonClient.connect()
      return daemonClient.listScheduledTaskRuns()
    },
    queryKey: RUNS_QUERY_KEY,
  })
  const workspacesQuery = useQuery({
    queryFn: async () => {
      const [defaultWorkspace, projectList] = await Promise.all([
        commandClient.getDefaultWorkspace(),
        commandClient.listProjects(),
      ])
      const options: WorkspaceOption[] = [
        { label: t('noWorkspace'), value: defaultWorkspace.path },
        ...projectList.projects
          .filter((project) => project.path !== defaultWorkspace.path)
          .map((project) => ({ label: project.name, value: project.path })),
      ]
      return { defaultRoot: defaultWorkspace.path, options }
    },
    queryKey: WORKSPACES_QUERY_KEY,
  })

  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: TASKS_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: RUNS_QUERY_KEY }),
    ])
  }
  const saveMutation = useMutation({
    mutationFn: (task: ScheduledTaskSpec) => daemonClient.saveScheduledTask(task),
    onSuccess: async () => {
      setFormOpen(false)
      setEditingTask(undefined)
      await invalidate()
    },
  })
  const toggleMutation = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      daemonClient.setScheduledTaskEnabled(id, enabled),
    onSuccess: invalidate,
  })
  const runMutation = useMutation({
    mutationFn: (id: string) => daemonClient.runScheduledTaskNow(id),
    onSuccess: invalidate,
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => daemonClient.deleteScheduledTask(id),
    onSuccess: async () => {
      setDeletingTask(undefined)
      await invalidate()
    },
  })

  const tasks = tasksQuery.data?.scheduledTasks ?? []
  const runs = runsQuery.data?.runs ?? []
  const lastRunByTask = useMemo(() => {
    const result = new Map<string, ScheduledTaskRunRecord>()
    for (const run of runs) {
      if (!result.has(run.scheduledTaskId)) result.set(run.scheduledTaskId, run)
    }
    return result
  }, [runs])
  const workspaceLabels = useMemo(
    () =>
      new Map(workspacesQuery.data?.options.map((option) => [option.value, option.label]) ?? []),
    [workspacesQuery.data?.options],
  )
  const filteredTasks = useMemo(() => {
    const normalizedSearch = search.trim().toLocaleLowerCase()
    return tasks.filter((task) => {
      const statusMatches =
        statusFilter === 'all' || (statusFilter === 'enabled' ? task.enabled : !task.enabled)
      const workspaceMatches =
        workspaceFilter === 'all' || (task.workspaceRoot ?? '') === workspaceFilter
      const searchMatches =
        !normalizedSearch ||
        task.name.toLocaleLowerCase().includes(normalizedSearch) ||
        task.prompt.toLocaleLowerCase().includes(normalizedSearch)
      return statusMatches && workspaceMatches && searchMatches
    })
  }, [search, statusFilter, tasks, workspaceFilter])

  const operationError =
    saveMutation.error ??
    toggleMutation.error ??
    runMutation.error ??
    deleteMutation.error ??
    runsQuery.error ??
    workspacesQuery.error

  function openCreateForm() {
    setEditingTask(undefined)
    setFormOpen(true)
  }

  function openEditForm(task: ScheduledTaskSpec) {
    setEditingTask(task)
    setFormOpen(true)
  }

  function openConversation(taskId: string) {
    void navigate({ search: { taskId: taskId as TypedUlid }, to: '/' })
  }

  return (
    <section aria-label={t('pageTitle')} className="h-full min-h-0 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5 pb-8">
        <header className="flex flex-col gap-4 border-border border-b pb-5 sm:flex-row sm:items-end sm:justify-between">
          <div className="space-y-1.5">
            <div className="flex items-center gap-3">
              <div className="flex size-9 items-center justify-center rounded-md border border-border bg-surface-raised text-muted-foreground">
                <CalendarClock aria-hidden="true" className="size-4.5" />
              </div>
              <h1 className="font-semibold text-2xl tracking-tight">{t('pageTitle')}</h1>
              <span className="rounded-full bg-muted px-2 py-0.5 text-muted-foreground text-xs">
                {t('count', { count: tasks.length })}
              </span>
            </div>
            <p className="max-w-2xl text-muted-foreground text-sm">{t('pageDescription')}</p>
          </div>
          <Button onClick={openCreateForm} type="button">
            <Plus aria-hidden="true" data-icon />
            {t('newTask')}
          </Button>
        </header>

        <div className="grid gap-2 sm:grid-cols-[minmax(220px,1fr)_180px_220px]">
          <div className="relative block">
            <Search
              aria-hidden="true"
              className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
            />
            <span className="sr-only">{t('searchPlaceholder')}</span>
            <Input
              aria-label={t('searchPlaceholder')}
              className="pl-9"
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('searchPlaceholder')}
              value={search}
            />
          </div>
          <Select
            aria-label={t('statusFilter')}
            onChange={(event) => setStatusFilter(event.target.value as StatusFilter)}
            value={statusFilter}
          >
            <option value="all">{t('allStatuses')}</option>
            <option value="enabled">{t('enabled')}</option>
            <option value="paused">{t('paused')}</option>
          </Select>
          <Select
            aria-label={t('workspaceFilter')}
            onChange={(event) => setWorkspaceFilter(event.target.value)}
            value={workspaceFilter}
          >
            <option value="all">{t('allWorkspaces')}</option>
            {workspacesQuery.data?.options.map((workspace) => (
              <option key={workspace.value} value={workspace.value}>
                {workspace.label}
              </option>
            ))}
          </Select>
        </div>

        {tasksQuery.isLoading ? (
          <div
            className="flex min-h-48 items-center justify-center text-muted-foreground text-sm"
            role="status"
          >
            <Clock3 aria-hidden="true" className="mr-2 size-4 animate-pulse" />
            {t('loading')}
          </div>
        ) : null}
        {tasksQuery.isError ? (
          <EmptyState className="flex min-h-48 flex-col items-center justify-center gap-3">
            <CircleAlert aria-hidden="true" className="size-5 text-destructive" />
            <p>{t('loadError')}</p>
            <Button
              onClick={() => void tasksQuery.refetch()}
              size="sm"
              type="button"
              variant="outline"
            >
              {t('retry')}
            </Button>
          </EmptyState>
        ) : null}
        {!tasksQuery.isLoading && !tasksQuery.isError && tasks.length === 0 ? (
          <EmptyState className="flex min-h-56 flex-col items-center justify-center gap-2">
            <CalendarClock aria-hidden="true" className="mb-2 size-6" />
            <h2 className="font-medium text-foreground">{t('emptyTitle')}</h2>
            <p>{t('emptyDescription')}</p>
            <Button className="mt-2" onClick={openCreateForm} size="sm" type="button">
              <Plus aria-hidden="true" data-icon />
              {t('newTask')}
            </Button>
          </EmptyState>
        ) : null}
        {!tasksQuery.isLoading && !tasksQuery.isError && tasks.length > 0 ? (
          <div className="overflow-hidden rounded-md border border-border bg-surface">
            <div className="hidden grid-cols-[minmax(220px,1.4fr)_100px_minmax(150px,.9fr)_130px_160px_130px_48px] gap-3 border-border border-b bg-muted/45 px-4 py-2.5 text-muted-foreground text-xs xl:grid">
              <span>{t('columns.task')}</span>
              <span>{t('columns.status')}</span>
              <span>{t('columns.workspace')}</span>
              <span>{t('columns.schedule')}</span>
              <span>{t('columns.nextRun')}</span>
              <span>{t('columns.lastRun')}</span>
              <span className="sr-only">{t('columns.actions')}</span>
            </div>
            {filteredTasks.length === 0 ? (
              <p className="px-4 py-12 text-center text-muted-foreground text-sm">
                {t('noMatches')}
              </p>
            ) : (
              filteredTasks.map((task) => {
                const taskRuns = runs.filter((run) => run.scheduledTaskId === task.id)
                const lastRun = lastRunByTask.get(task.id)
                const expanded = expandedTaskId === task.id
                const workspaceLabel = task.workspaceRoot
                  ? (workspaceLabels.get(task.workspaceRoot) ?? task.workspaceRoot)
                  : t('noWorkspace')
                return (
                  <ScheduledTaskRow
                    busy={
                      toggleMutation.isPending || runMutation.isPending || deleteMutation.isPending
                    }
                    expanded={expanded}
                    key={task.id}
                    lastRun={lastRun}
                    onDelete={() => setDeletingTask(task)}
                    onEdit={() => openEditForm(task)}
                    onOpenConversation={openConversation}
                    onRun={() => runMutation.mutate(task.id)}
                    onToggle={() => toggleMutation.mutate({ enabled: !task.enabled, id: task.id })}
                    onToggleDetails={() => setExpandedTaskId(expanded ? undefined : task.id)}
                    runs={taskRuns}
                    task={task}
                    workspaceLabel={workspaceLabel}
                  />
                )
              })
            )}
          </div>
        ) : null}

        {operationError ? (
          <p aria-live="polite" className="text-destructive text-sm" role="status">
            {operationError instanceof Error ? operationError.message : t('errors.operation')}
          </p>
        ) : null}
      </div>

      <ScheduledTaskFormDialog
        defaultWorkspaceRoot={workspacesQuery.data?.defaultRoot}
        key={editingTask?.id ?? 'new'}
        onOpenChange={setFormOpen}
        onSave={(task) => saveMutation.mutate(task)}
        open={formOpen}
        saving={saveMutation.isPending}
        task={editingTask}
        workspaces={workspacesQuery.data?.options ?? []}
      />
      <DeleteScheduledTaskDialog
        deleting={deleteMutation.isPending}
        onConfirm={() => deletingTask && deleteMutation.mutate(deletingTask.id)}
        onOpenChange={(open) => {
          if (!open) setDeletingTask(undefined)
        }}
        task={deletingTask}
      />
    </section>
  )
}

function ScheduledTaskRow({
  busy,
  expanded,
  lastRun,
  onDelete,
  onEdit,
  onOpenConversation,
  onRun,
  onToggle,
  onToggleDetails,
  runs,
  task,
  workspaceLabel,
}: {
  busy: boolean
  expanded: boolean
  lastRun?: ScheduledTaskRunRecord
  onDelete: () => void
  onEdit: () => void
  onOpenConversation: (taskId: string) => void
  onRun: () => void
  onToggle: () => void
  onToggleDetails: () => void
  runs: ScheduledTaskRunRecord[]
  task: ScheduledTaskSpec
  workspaceLabel: string
}) {
  const { t } = useTranslation('scheduledTasks')
  const nextRun = task.enabled ? nextRunAt(task, lastRun) : undefined

  const menu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          aria-label={t('actions.more', { name: task.name })}
          size="icon"
          type="button"
          variant="ghost"
        >
          <MoreHorizontal aria-hidden="true" data-icon />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem disabled={busy} onSelect={onRun}>
          <Play aria-hidden="true" className="size-4" />
          {t('actions.runNow')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onEdit}>
          <Pencil aria-hidden="true" className="size-4" />
          {t('actions.edit')}
        </DropdownMenuItem>
        <DropdownMenuItem disabled={busy} onSelect={onToggle}>
          <CalendarClock aria-hidden="true" className="size-4" />
          {task.enabled ? t('actions.pause') : t('actions.enable')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={onDelete}>
          <Trash2 aria-hidden="true" className="size-4" />
          {t('actions.delete')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )

  return (
    <article className="border-border border-b last:border-b-0">
      <div className="hidden grid-cols-[minmax(220px,1.4fr)_100px_minmax(150px,.9fr)_130px_160px_130px_48px] items-center gap-3 px-4 py-3 xl:grid">
        <button className="min-w-0 text-left" onClick={onToggleDetails} type="button">
          <span className="flex items-center gap-2 font-medium text-sm">
            {expanded ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
            <span className="truncate">{task.name}</span>
          </span>
          <span className="mt-1 block truncate pl-5.5 text-muted-foreground text-xs">
            {safePromptPreview(task.prompt)}
          </span>
        </button>
        <StatusBadge tone={task.enabled ? 'success' : 'neutral'}>
          {task.enabled ? t('enabled') : t('paused')}
        </StatusBadge>
        <span className="truncate text-sm" title={workspaceLabel}>
          {workspaceLabel}
        </span>
        <span className="text-sm">
          {t('everyMinutes', { count: task.schedule.intervalMinutes })}
        </span>
        <span className="text-muted-foreground text-sm">
          {nextRun ? formatDate(nextRun) : t('nextRunUnavailable')}
        </span>
        <LastRun run={lastRun} />
        {menu}
      </div>

      <div className="space-y-3 p-4 xl:hidden">
        <div className="flex items-start justify-between gap-3">
          <button className="min-w-0 text-left" onClick={onToggleDetails} type="button">
            <span className="flex items-center gap-2 font-medium">
              {expanded ? <ChevronUp className="size-4" /> : <ChevronDown className="size-4" />}
              <span className="truncate">{task.name}</span>
            </span>
            <span className="mt-1 block line-clamp-2 pl-6 text-muted-foreground text-sm">
              {safePromptPreview(task.prompt)}
            </span>
          </button>
          {menu}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <StatusBadge tone={task.enabled ? 'success' : 'neutral'}>
            {task.enabled ? t('enabled') : t('paused')}
          </StatusBadge>
          <span className="rounded-full border border-border px-2 py-0.5 text-muted-foreground text-xs">
            {t('everyMinutes', { count: task.schedule.intervalMinutes })}
          </span>
          <span className="truncate text-muted-foreground text-xs">{workspaceLabel}</span>
        </div>
        <div className="grid grid-cols-2 gap-3 text-xs">
          <div>
            <span className="block text-muted-foreground">{t('columns.nextRun')}</span>
            <span className="mt-1 block">
              {nextRun ? formatDate(nextRun) : t('nextRunUnavailable')}
            </span>
          </div>
          <div>
            <span className="block text-muted-foreground">{t('columns.lastRun')}</span>
            <span className="mt-1 block">
              <LastRun run={lastRun} />
            </span>
          </div>
        </div>
      </div>

      {expanded ? (
        <TaskDetails onOpenConversation={onOpenConversation} runs={runs} task={task} />
      ) : null}
    </article>
  )
}

function TaskDetails({
  onOpenConversation,
  runs,
  task,
}: {
  onOpenConversation: (taskId: string) => void
  runs: ScheduledTaskRunRecord[]
  task: ScheduledTaskSpec
}) {
  const { t } = useTranslation('scheduledTasks')
  return (
    <div className="grid gap-5 border-border border-t bg-muted/20 px-4 py-4 lg:grid-cols-[minmax(0,1fr)_minmax(300px,.8fr)]">
      <div className="space-y-4">
        <div>
          <h3 className="mb-1.5 font-medium text-xs uppercase tracking-wide text-muted-foreground">
            {t('details.prompt')}
          </h3>
          <p className="whitespace-pre-wrap rounded-md border border-border bg-background p-3 text-sm leading-6">
            {safePromptPreview(task.prompt)}
          </p>
        </div>
        <dl className="grid gap-3 sm:grid-cols-2">
          <Detail label={t('details.permissionMode')}>
            {t(`permissionMode.${task.permissionMode}`)}
          </Detail>
          <Detail label={t('details.missedRunPolicy')}>
            {t(`missedRunPolicy.${task.missedRunPolicy ?? 'skip'}`)}
          </Detail>
        </dl>
      </div>
      <div>
        <h3 className="mb-2 font-medium text-xs uppercase tracking-wide text-muted-foreground">
          {t('details.recentRuns')}
        </h3>
        {runs.length === 0 ? (
          <p className="text-muted-foreground text-sm">{t('details.noRuns')}</p>
        ) : (
          <ol className="space-y-1.5">
            {runs.slice(0, 6).map((run) => (
              <li
                className="flex items-center justify-between gap-3 rounded-md border border-border bg-background px-3 py-2"
                key={run.id}
              >
                <div className="min-w-0">
                  <StatusBadge tone={runStatusTone(run.status)}>
                    {t(`runStatus.${run.status}`)}
                  </StatusBadge>
                  <span className="ml-2 text-muted-foreground text-xs">
                    {formatDate(run.startedAt)}
                  </span>
                </div>
                {run.taskId ? (
                  <Button
                    onClick={() => {
                      if (run.taskId) onOpenConversation(run.taskId)
                    }}
                    size="sm"
                    type="button"
                    variant="ghost"
                  >
                    {t('actions.openConversation')}
                  </Button>
                ) : null}
              </li>
            ))}
          </ol>
        )}
      </div>
    </div>
  )
}

function ScheduledTaskFormDialog({
  defaultWorkspaceRoot,
  onOpenChange,
  onSave,
  open,
  saving,
  task,
  workspaces,
}: {
  defaultWorkspaceRoot?: string
  onOpenChange: (open: boolean) => void
  onSave: (task: ScheduledTaskSpec) => void
  open: boolean
  saving: boolean
  task?: ScheduledTaskSpec
  workspaces: WorkspaceOption[]
}) {
  const { t } = useTranslation('scheduledTasks')
  const [permissionMode, setPermissionMode] = useState<PermissionMode>(
    task?.permissionMode ?? 'default',
  )
  const [formError, setFormError] = useState<string>()
  const saveEnabledRef = useRef(task?.enabled ?? true)

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    const name = String(data.get('name') ?? '').trim()
    const prompt = String(data.get('prompt') ?? '').trim()
    const intervalMinutes = Number(data.get('intervalMinutes'))
    if (!name) return setFormError(t('errors.nameRequired'))
    if (!prompt) return setFormError(t('errors.promptRequired'))
    if (hasObviousUnredactedSecret(prompt)) return setFormError(t('errors.secretPrompt'))
    if (!Number.isSafeInteger(intervalMinutes) || intervalMinutes <= 0) {
      return setFormError(t('errors.intervalRequired'))
    }
    const now = new Date().toISOString()
    onSave({
      createdAt: task?.createdAt ?? now,
      enabled: saveEnabledRef.current,
      id: task?.id ?? newScheduledTaskId(),
      missedRunPolicy: String(data.get('missedRunPolicy')) as 'skip' | 'run_once',
      name,
      permissionMode,
      prompt,
      schedule: { intervalMinutes },
      updatedAt: now,
      workspaceRoot: String(data.get('workspaceRoot') ?? '') || undefined,
    })
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        setFormError(undefined)
        onOpenChange(nextOpen)
      }}
      open={open}
    >
      <DialogContent className="max-h-[calc(100vh-2rem)] w-[min(calc(100vw-2rem),42rem)] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{task ? t('form.editTitle') : t('form.createTitle')}</DialogTitle>
          <DialogDescription>{t('form.description')}</DialogDescription>
        </DialogHeader>
        <form className="space-y-4" key={task?.id ?? 'new'} onSubmit={submit}>
          <div className="grid gap-4 sm:grid-cols-2">
            <FieldControl fieldId="scheduled-task-name" label={t('form.name')}>
              <Input
                autoFocus
                defaultValue={task?.name}
                id="scheduled-task-name"
                name="name"
                placeholder={t('form.namePlaceholder')}
              />
            </FieldControl>
            <FieldControl fieldId="scheduled-task-workspace" label={t('form.workspace')}>
              <Select
                defaultValue={task?.workspaceRoot ?? defaultWorkspaceRoot ?? ''}
                id="scheduled-task-workspace"
                name="workspaceRoot"
              >
                {workspaces.map((workspace) => (
                  <option key={workspace.value} value={workspace.value}>
                    {workspace.label}
                  </option>
                ))}
              </Select>
            </FieldControl>
          </div>
          <FieldControl fieldId="scheduled-task-prompt" label={t('form.prompt')}>
            <Textarea
              className="min-h-32"
              defaultValue={task?.prompt}
              id="scheduled-task-prompt"
              name="prompt"
              placeholder={t('form.promptPlaceholder')}
            />
          </FieldControl>
          <div className="grid gap-4 sm:grid-cols-3">
            <FieldControl fieldId="scheduled-task-interval" label={t('form.interval')}>
              <Input
                defaultValue={task?.schedule.intervalMinutes ?? 60}
                id="scheduled-task-interval"
                min={1}
                name="intervalMinutes"
                step={1}
                type="number"
              />
            </FieldControl>
            <FieldControl fieldId="scheduled-task-permission" label={t('form.permissionMode')}>
              <Select
                id="scheduled-task-permission"
                name="permissionMode"
                onChange={(event) => setPermissionMode(event.target.value as PermissionMode)}
                value={permissionMode}
              >
                <option value="default">{t('permissionMode.default')}</option>
                <option value="auto">{t('permissionMode.auto')}</option>
                <option value="bypass_permissions">{t('permissionMode.bypass_permissions')}</option>
              </Select>
            </FieldControl>
            <FieldControl fieldId="scheduled-task-missed" label={t('form.missedRunPolicy')}>
              <Select
                defaultValue={task?.missedRunPolicy ?? 'skip'}
                id="scheduled-task-missed"
                name="missedRunPolicy"
              >
                <option value="skip">{t('missedRunPolicy.skip')}</option>
                <option value="run_once">{t('missedRunPolicy.run_once')}</option>
              </Select>
            </FieldControl>
          </div>
          {permissionMode === 'bypass_permissions' ? (
            <p className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning">
              {t('form.fullAccessWarning')}
            </p>
          ) : null}
          {formError ? <p className="text-destructive text-sm">{formError}</p> : null}
          <DialogFooter>
            <Button onClick={() => onOpenChange(false)} type="button" variant="ghost">
              {t('form.cancel')}
            </Button>
            <Button
              disabled={saving}
              onClick={() => {
                saveEnabledRef.current = false
              }}
              type="submit"
              variant="outline"
            >
              {saving ? t('form.saving') : t('form.savePaused')}
            </Button>
            <Button
              disabled={saving}
              onClick={() => {
                saveEnabledRef.current = true
              }}
              type="submit"
            >
              {saving ? t('form.saving') : t('form.saveEnabled')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function DeleteScheduledTaskDialog({
  deleting,
  onConfirm,
  onOpenChange,
  task,
}: {
  deleting: boolean
  onConfirm: () => void
  onOpenChange: (open: boolean) => void
  task?: ScheduledTaskSpec
}) {
  const { t } = useTranslation('scheduledTasks')
  return (
    <Dialog onOpenChange={onOpenChange} open={Boolean(task)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('deleteDialog.title', { name: task?.name })}</DialogTitle>
          <DialogDescription>{t('deleteDialog.description')}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)} type="button" variant="ghost">
            {t('deleteDialog.cancel')}
          </Button>
          <Button disabled={deleting} onClick={onConfirm} type="button" variant="destructive">
            {t('deleteDialog.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function Detail({ children, label }: { children: string; label: string }) {
  return (
    <div>
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="mt-1 text-sm">{children}</dd>
    </div>
  )
}

function LastRun({ run }: { run?: ScheduledTaskRunRecord }) {
  const { t } = useTranslation('scheduledTasks')
  return run ? (
    <StatusBadge tone={runStatusTone(run.status)}>{t(`runStatus.${run.status}`)}</StatusBadge>
  ) : (
    <span className="text-muted-foreground text-sm">{t('notRun')}</span>
  )
}

function runStatusTone(status: ScheduledTaskRunStatus): StatusBadgeProps['tone'] {
  if (status === 'succeeded') return 'success'
  if (status === 'failed' || status === 'rejected') return 'destructive'
  if (status === 'started') return 'info'
  return 'neutral'
}

function nextRunAt(task: ScheduledTaskSpec, lastRun?: ScheduledTaskRunRecord) {
  const baseline = Math.max(
    new Date(task.updatedAt).getTime(),
    lastRun ? new Date(lastRun.startedAt).getTime() : 0,
  )
  return new Date(baseline + task.schedule.intervalMinutes * 60_000)
}

function safePromptPreview(prompt: string) {
  return hasObviousUnredactedSecret(prompt) ? '••••••••' : prompt
}

function formatDate(value: Date | string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}

function newScheduledTaskId() {
  return globalThis.crypto?.randomUUID?.() ?? `scheduled-${Date.now().toString(36)}`
}
