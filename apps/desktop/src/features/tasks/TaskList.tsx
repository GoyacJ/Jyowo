import type { TFunction } from 'i18next'
import {
  Archive,
  ArrowDown,
  ArrowUp,
  CalendarClock,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CirclePause,
  Folder,
  LoaderCircle,
  MessageSquareMore,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Search,
  Trash2,
} from 'lucide-react'
import { type FormEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { TaskProjection, TypedUlid } from '@/generated/daemon-protocol'
import { cn } from '@/shared/lib/utils'
import type { SidebarSection, SidebarSections } from '@/shared/state/ui-store'
import type { ListProjectsResponse, MoveProjectDirection } from '@/shared/tauri/commands'
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

type Project = ListProjectsResponse['projects'][number]
type MaybePromise = void | Promise<void>

export type SidebarTaskGroups = {
  pinned: TaskProjection[]
  projects: Array<{ project: Project; tasks: TaskProjection[] }>
  conversations: TaskProjection[]
}

export function groupSidebarTasks(
  tasks: TaskProjection[],
  projects: Project[],
  defaultRoot: string,
): SidebarTaskGroups {
  const visible = tasks
    .filter((task) => !task.parent && !task.removed && !task.archived)
    .sort(
      (left, right) =>
        right.lastGlobalOffset - left.lastGlobalOffset || left.title.localeCompare(right.title),
    )
  const pinned = visible.filter((task) => task.pinned)
  const unpinned = visible.filter((task) => !task.pinned)
  const projectPaths = new Set(projects.map((project) => project.path))

  return {
    pinned,
    projects: projects.map((project) => ({
      project,
      tasks: unpinned.filter((task) => task.workspace?.root === project.path),
    })),
    conversations: unpinned.filter((task) => {
      const root = task.workspace?.root
      return !root || root === defaultRoot || !projectPaths.has(root)
    }),
  }
}

export function TaskList({
  activeTaskId,
  compact = false,
  creating = false,
  defaultRoot,
  expandedProjects,
  loading = false,
  onAddProject,
  onCreateConversation,
  onOpenGlobalSearch,
  onMoveProject,
  onRemoveProject,
  onRemoveTask,
  onRenameProject,
  onRenameTask,
  onOpenScheduledTasks,
  onSelectTask,
  onSetTaskArchived,
  onSetTaskPinned,
  onToggleProject,
  onToggleSection,
  projects,
  sections,
  scheduledTasksActive = false,
  tasks,
}: {
  activeTaskId?: TypedUlid
  compact?: boolean
  creating?: boolean
  defaultRoot: string
  expandedProjects: Record<string, boolean>
  loading?: boolean
  onAddProject: () => MaybePromise
  onCreateConversation: (root: string) => MaybePromise
  onOpenGlobalSearch: () => void
  onMoveProject: (path: string, direction: MoveProjectDirection) => MaybePromise
  onRemoveProject: (path: string) => MaybePromise
  onRemoveTask: (task: TaskProjection) => MaybePromise
  onRenameProject: (path: string, name: string) => MaybePromise
  onRenameTask: (task: TaskProjection, title: string) => MaybePromise
  onOpenScheduledTasks: () => void
  onSelectTask: (taskId: TypedUlid) => void
  onSetTaskArchived: (task: TaskProjection, archived: boolean) => MaybePromise
  onSetTaskPinned: (task: TaskProjection, pinned: boolean) => MaybePromise
  onToggleProject: (path: string, expanded: boolean) => void
  onToggleSection: (section: SidebarSection, expanded: boolean) => void
  projects: Project[]
  sections: SidebarSections
  scheduledTasksActive?: boolean
  tasks: TaskProjection[]
}) {
  const { t } = useTranslation('shell')
  const groups = groupSidebarTasks(tasks, projects, defaultRoot)

  return (
    <nav aria-label={t('sidebar.navigationLabel')} className="flex min-h-0 flex-1 flex-col">
      <div className="space-y-1 px-2 py-2">
        <button
          aria-label={t('actions.openGlobalSearch')}
          className={cn(
            'flex h-9 w-full items-center rounded-md text-muted-foreground text-sm transition-colors hover:bg-row-muted hover:text-foreground',
            compact ? 'justify-center px-0' : 'gap-2 px-2.5',
          )}
          onClick={onOpenGlobalSearch}
          type="button"
        >
          <Search aria-hidden="true" className="size-4" />
          {compact ? null : (
            <>
              <span>{t('nav.globalSearch')}</span>
              <kbd className="ml-auto font-sans text-[11px] text-muted-foreground">⌘ K</kbd>
            </>
          )}
        </button>
        <button
          aria-label={t('actions.newConversation')}
          className={cn(
            'flex h-9 w-full items-center rounded-md border border-border bg-surface-raised text-sm transition-colors hover:bg-row-muted disabled:opacity-50',
            compact ? 'justify-center px-0' : 'gap-2 px-2.5',
          )}
          disabled={creating || !defaultRoot}
          onClick={() => onCreateConversation(defaultRoot)}
          type="button"
        >
          {creating ? (
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          ) : (
            <Plus aria-hidden="true" className="size-4" />
          )}
          {compact ? null : (
            <span>{creating ? t('conversations.loading') : t('actions.newConversation')}</span>
          )}
        </button>
        <button
          aria-current={scheduledTasksActive ? 'page' : undefined}
          aria-label={t('actions.openScheduledTasks')}
          className={cn(
            'flex h-9 w-full items-center rounded-md text-sm transition-colors hover:bg-row-muted',
            scheduledTasksActive
              ? 'bg-row-muted font-medium text-foreground'
              : 'text-muted-foreground',
            compact ? 'justify-center px-0' : 'gap-2 px-2.5',
          )}
          onClick={onOpenScheduledTasks}
          type="button"
        >
          <CalendarClock aria-hidden="true" className="size-4" />
          {compact ? null : <span>{t('nav.scheduledTasks')}</span>}
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
        {loading ? (
          <p className="px-2 py-2 text-muted-foreground text-xs" role="status">
            {t('sidebar.loading')}
          </p>
        ) : null}

        <SidebarSectionView
          compact={compact}
          expanded={sections.pinned}
          label={t('sections.pinned')}
          onToggle={() => onToggleSection('pinned', !sections.pinned)}
        >
          <TaskRows
            activeTaskId={activeTaskId}
            compact={compact}
            emptyLabel={t('sidebar.emptyPinned')}
            onRemoveTask={onRemoveTask}
            onRenameTask={onRenameTask}
            onSelectTask={onSelectTask}
            onSetTaskArchived={onSetTaskArchived}
            onSetTaskPinned={onSetTaskPinned}
            tasks={groups.pinned}
          />
        </SidebarSectionView>

        <SidebarSectionView
          action={
            <button
              aria-label={t('sidebar.addProject')}
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
              onClick={() => onAddProject()}
              type="button"
            >
              <Plus aria-hidden="true" className="size-3.5" />
            </button>
          }
          compact={compact}
          expanded={sections.projects}
          label={t('sections.projects')}
          onToggle={() => onToggleSection('projects', !sections.projects)}
        >
          <div className="space-y-0.5">
            {groups.projects.length === 0 ? (
              <EmptyRow compact={compact}>{t('sidebar.emptyProjects')}</EmptyRow>
            ) : (
              groups.projects.map(({ project, tasks: projectTasks }, index) => {
                const expanded = expandedProjects[project.path] ?? true
                return (
                  <ProjectRow
                    activeTaskId={activeTaskId}
                    compact={compact}
                    expanded={expanded}
                    first={index === 0}
                    key={project.path}
                    last={index === groups.projects.length - 1}
                    onCreateConversation={onCreateConversation}
                    onMoveProject={onMoveProject}
                    onRemoveProject={onRemoveProject}
                    onRemoveTask={onRemoveTask}
                    onRenameProject={onRenameProject}
                    onRenameTask={onRenameTask}
                    onSelectTask={onSelectTask}
                    onSetTaskArchived={onSetTaskArchived}
                    onSetTaskPinned={onSetTaskPinned}
                    onToggle={() => onToggleProject(project.path, !expanded)}
                    project={project}
                    tasks={projectTasks}
                  />
                )
              })
            )}
          </div>
        </SidebarSectionView>

        <SidebarSectionView
          compact={compact}
          expanded={sections.conversations}
          label={t('sections.conversations')}
          onToggle={() => onToggleSection('conversations', !sections.conversations)}
        >
          <TaskRows
            activeTaskId={activeTaskId}
            compact={compact}
            emptyLabel={t('sidebar.emptyConversations')}
            onRemoveTask={onRemoveTask}
            onRenameTask={onRenameTask}
            onSelectTask={onSelectTask}
            onSetTaskArchived={onSetTaskArchived}
            onSetTaskPinned={onSetTaskPinned}
            tasks={groups.conversations}
          />
        </SidebarSectionView>
      </div>
    </nav>
  )
}

function SidebarSectionView({
  action,
  children,
  compact,
  expanded,
  label,
  onToggle,
}: {
  action?: React.ReactNode
  children: React.ReactNode
  compact: boolean
  expanded: boolean
  label: string
  onToggle: () => void
}) {
  if (compact) {
    return expanded ? <section aria-label={label}>{children}</section> : null
  }
  return (
    <section aria-label={label} className="mb-3">
      <div className="flex items-center">
        <button
          aria-expanded={expanded}
          className="group flex h-7 min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 font-medium text-[11px] text-muted-foreground uppercase tracking-[0.08em] hover:bg-muted hover:text-foreground"
          onClick={onToggle}
          type="button"
        >
          {expanded ? (
            <ChevronDown aria-hidden="true" className="size-3.5" />
          ) : (
            <ChevronRight aria-hidden="true" className="size-3.5" />
          )}
          <span>{label}</span>
        </button>
        {action}
      </div>
      {expanded ? children : null}
    </section>
  )
}

function ProjectRow({
  activeTaskId,
  compact,
  expanded,
  first,
  last,
  onCreateConversation,
  onMoveProject,
  onRemoveProject,
  onRemoveTask,
  onRenameProject,
  onRenameTask,
  onSelectTask,
  onSetTaskArchived,
  onSetTaskPinned,
  onToggle,
  project,
  tasks,
}: {
  activeTaskId?: TypedUlid
  compact: boolean
  expanded: boolean
  first: boolean
  last: boolean
  onCreateConversation: (root: string) => MaybePromise
  onMoveProject: (path: string, direction: MoveProjectDirection) => MaybePromise
  onRemoveProject: (path: string) => MaybePromise
  onRemoveTask: (task: TaskProjection) => MaybePromise
  onRenameProject: (path: string, name: string) => MaybePromise
  onRenameTask: (task: TaskProjection, title: string) => MaybePromise
  onSelectTask: (taskId: TypedUlid) => void
  onSetTaskArchived: (task: TaskProjection, archived: boolean) => MaybePromise
  onSetTaskPinned: (task: TaskProjection, pinned: boolean) => MaybePromise
  onToggle: () => void
  project: Project
  tasks: TaskProjection[]
}) {
  const { t } = useTranslation('shell')
  const [renameOpen, setRenameOpen] = useState(false)
  const [removeOpen, setRemoveOpen] = useState(false)

  if (compact) {
    return (
      <div className="py-0.5">
        <button
          aria-label={project.name}
          className="flex h-9 w-full items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onToggle}
          title={project.name}
          type="button"
        >
          <Folder aria-hidden="true" className="size-4" />
        </button>
        {expanded ? (
          <TaskRows
            activeTaskId={activeTaskId}
            compact
            emptyLabel={t('sidebar.emptyConversations')}
            onRemoveTask={onRemoveTask}
            onRenameTask={onRenameTask}
            onSelectTask={onSelectTask}
            onSetTaskArchived={onSetTaskArchived}
            onSetTaskPinned={onSetTaskPinned}
            tasks={tasks}
          />
        ) : null}
      </div>
    )
  }

  return (
    <div>
      <div className="group flex h-8 items-center rounded-md hover:bg-muted">
        <button
          aria-expanded={expanded}
          aria-label={project.name}
          className="flex min-w-0 flex-1 items-center gap-1.5 px-1.5 text-left text-[13px]"
          onClick={onToggle}
          type="button"
        >
          {expanded ? (
            <ChevronDown aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronRight aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
          )}
          <Folder aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
          <span className="truncate">{project.name}</span>
        </button>
        <button
          aria-label={t('sidebar.newInProject', { name: project.name })}
          className="flex size-7 shrink-0 items-center justify-center rounded opacity-0 hover:bg-background group-hover:opacity-100 focus-visible:opacity-100"
          onClick={() => onCreateConversation(project.path)}
          type="button"
        >
          <Plus aria-hidden="true" className="size-3.5" />
        </button>
        <ProjectActions
          first={first}
          last={last}
          onMoveProject={onMoveProject}
          onRemove={() => setRemoveOpen(true)}
          onRename={() => setRenameOpen(true)}
          project={project}
        />
      </div>
      {expanded ? (
        <section aria-label={`${project.name} conversations`} className="pl-4">
          <TaskRows
            activeTaskId={activeTaskId}
            compact={false}
            emptyLabel={t('sidebar.emptyConversations')}
            onRemoveTask={onRemoveTask}
            onRenameTask={onRenameTask}
            onSelectTask={onSelectTask}
            onSetTaskArchived={onSetTaskArchived}
            onSetTaskPinned={onSetTaskPinned}
            tasks={tasks}
          />
        </section>
      ) : null}
      <RenameDialog
        description={t('sidebar.renameProjectDescription')}
        initialValue={project.name}
        label={t('sidebar.projectName')}
        onOpenChange={setRenameOpen}
        onSave={(name) => onRenameProject(project.path, name)}
        open={renameOpen}
        title={t('sidebar.renameProject')}
      />
      <ConfirmDialog
        confirmLabel={t('sidebar.removeProject')}
        description={t('sidebar.removeProjectDescription')}
        onConfirm={() => onRemoveProject(project.path)}
        onOpenChange={setRemoveOpen}
        open={removeOpen}
        title={t('sidebar.removeProjectTitle', { name: project.name })}
      />
    </div>
  )
}

function ProjectActions({
  first,
  last,
  onMoveProject,
  onRemove,
  onRename,
  project,
}: {
  first: boolean
  last: boolean
  onMoveProject: (path: string, direction: MoveProjectDirection) => MaybePromise
  onRemove: () => void
  onRename: () => void
  project: Project
}) {
  const { t } = useTranslation('shell')
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          aria-label={t('sidebar.actions', { name: project.name })}
          className="mr-0.5 flex size-7 shrink-0 items-center justify-center rounded opacity-0 hover:bg-background group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:bg-background data-[state=open]:opacity-100"
          type="button"
        >
          <MoreHorizontal aria-hidden="true" className="size-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="right">
        <DropdownMenuItem onSelect={onRename}>
          <Pencil aria-hidden="true" className="size-4" /> {t('sidebar.rename')}
        </DropdownMenuItem>
        <DropdownMenuItem disabled={first} onSelect={() => onMoveProject(project.path, 'up')}>
          <ArrowUp aria-hidden="true" className="size-4" /> {t('sidebar.moveUp')}
        </DropdownMenuItem>
        <DropdownMenuItem disabled={last} onSelect={() => onMoveProject(project.path, 'down')}>
          <ArrowDown aria-hidden="true" className="size-4" /> {t('sidebar.moveDown')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={onRemove}>
          <Trash2 aria-hidden="true" className="size-4" /> {t('sidebar.removeProject')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function TaskRows({
  activeTaskId,
  compact,
  emptyLabel,
  onRemoveTask,
  onRenameTask,
  onSelectTask,
  onSetTaskArchived,
  onSetTaskPinned,
  tasks,
}: {
  activeTaskId?: TypedUlid
  compact: boolean
  emptyLabel: string
  onRemoveTask: (task: TaskProjection) => MaybePromise
  onRenameTask: (task: TaskProjection, title: string) => MaybePromise
  onSelectTask: (taskId: TypedUlid) => void
  onSetTaskArchived: (task: TaskProjection, archived: boolean) => MaybePromise
  onSetTaskPinned: (task: TaskProjection, pinned: boolean) => MaybePromise
  tasks: TaskProjection[]
}) {
  if (tasks.length === 0) return <EmptyRow compact={compact}>{emptyLabel}</EmptyRow>
  return (
    <ul className="space-y-0.5">
      {tasks.map((task) => (
        <TaskRow
          active={task.taskId === activeTaskId}
          compact={compact}
          key={task.taskId}
          onRemoveTask={onRemoveTask}
          onRenameTask={onRenameTask}
          onSelectTask={onSelectTask}
          onSetTaskArchived={onSetTaskArchived}
          onSetTaskPinned={onSetTaskPinned}
          task={task}
        />
      ))}
    </ul>
  )
}

function TaskRow({
  active,
  compact,
  onRemoveTask,
  onRenameTask,
  onSelectTask,
  onSetTaskArchived,
  onSetTaskPinned,
  task,
}: {
  active: boolean
  compact: boolean
  onRemoveTask: (task: TaskProjection) => MaybePromise
  onRenameTask: (task: TaskProjection, title: string) => MaybePromise
  onSelectTask: (taskId: TypedUlid) => void
  onSetTaskArchived: (task: TaskProjection, archived: boolean) => MaybePromise
  onSetTaskPinned: (task: TaskProjection, pinned: boolean) => MaybePromise
  task: TaskProjection
}) {
  const { t } = useTranslation('shell')
  const [editingTitle, setEditingTitle] = useState(false)
  const [titleDraft, setTitleDraft] = useState(task.title)
  const titleInputRef = useRef<HTMLInputElement>(null)
  const [renameOpen, setRenameOpen] = useState(false)
  const [archiveOpen, setArchiveOpen] = useState(false)
  const [removeOpen, setRemoveOpen] = useState(false)
  const status = taskStatus(task, t)

  useEffect(() => {
    if (!editingTitle) return
    setTitleDraft(task.title)
    titleInputRef.current?.focus()
    titleInputRef.current?.select()
  }, [editingTitle, task.title])

  function saveInlineTitle() {
    const title = titleDraft.trim()
    setEditingTitle(false)
    if (title && title !== task.title) void onRenameTask(task, title)
  }

  return (
    <li>
      <div
        className={cn(
          'group flex items-center rounded-md hover:bg-muted',
          compact ? 'h-9 justify-center' : 'min-h-8',
          active && 'bg-selection text-foreground',
        )}
      >
        {editingTitle ? (
          <div className="flex min-w-0 flex-1 items-center gap-2 px-1.5 py-1">
            <TaskStatusIcon status={status.key} />
            <input
              aria-label={t('sidebar.conversationName')}
              className="h-6 min-w-0 flex-1 rounded border border-input bg-background px-1.5 text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring"
              maxLength={120}
              onBlur={saveInlineTitle}
              onChange={(event) => setTitleDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  saveInlineTitle()
                } else if (event.key === 'Escape') {
                  event.preventDefault()
                  setEditingTitle(false)
                }
              }}
              ref={titleInputRef}
              value={titleDraft}
            />
          </div>
        ) : (
          <button
            aria-current={active ? 'page' : undefined}
            aria-label={compact ? `${task.title}, ${status.label}` : undefined}
            className={cn(
              'flex min-w-0 flex-1 items-center text-left',
              compact ? 'h-9 justify-center' : 'gap-2 px-1.5 py-1.5',
            )}
            onClick={() => onSelectTask(task.taskId)}
            onDoubleClick={() => {
              if (!compact) setEditingTitle(true)
            }}
            title={compact ? task.title : status.label}
            type="button"
          >
            <TaskStatusIcon status={status.key} />
            {compact ? null : <span className="truncate text-[13px]">{task.title}</span>}
          </button>
        )}
        {compact || editingTitle ? null : (
          <>
            <button
              aria-label={`${task.pinned ? t('sidebar.unpin') : t('sidebar.pin')} ${task.title}`}
              className="flex size-7 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 hover:bg-background hover:text-foreground group-hover:opacity-100 focus-visible:opacity-100"
              onClick={() => onSetTaskPinned(task, !task.pinned)}
              title={task.pinned ? t('sidebar.unpin') : t('sidebar.pin')}
              type="button"
            >
              {task.pinned ? (
                <PinOff aria-hidden="true" className="size-4" />
              ) : (
                <Pin aria-hidden="true" className="size-4" />
              )}
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  aria-label={t('sidebar.actions', { name: task.title })}
                  className="flex size-7 shrink-0 items-center justify-center rounded opacity-0 hover:bg-background group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:bg-background data-[state=open]:opacity-100"
                  type="button"
                >
                  <MoreHorizontal aria-hidden="true" className="size-4" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" side="right">
                <DropdownMenuItem onSelect={() => onSetTaskPinned(task, !task.pinned)}>
                  {task.pinned ? (
                    <PinOff aria-hidden="true" className="size-4" />
                  ) : (
                    <Pin aria-hidden="true" className="size-4" />
                  )}
                  {task.pinned ? t('sidebar.unpin') : t('sidebar.pin')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setRenameOpen(true)}>
                  <Pencil aria-hidden="true" className="size-4" /> {t('sidebar.rename')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setArchiveOpen(true)}>
                  <Archive aria-hidden="true" className="size-4" /> {t('sidebar.archive')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="text-destructive focus:text-destructive"
                  onSelect={() => setRemoveOpen(true)}
                >
                  <Trash2 aria-hidden="true" className="size-4" /> {t('sidebar.remove')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <button
              aria-label={`${t('sidebar.remove')} ${task.title}`}
              className="mr-0.5 flex size-7 shrink-0 items-center justify-center rounded text-muted-foreground opacity-0 hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100 focus-visible:opacity-100"
              onClick={() => onRemoveTask(task)}
              title={t('sidebar.remove')}
              type="button"
            >
              <Trash2 aria-hidden="true" className="size-4" />
            </button>
          </>
        )}
      </div>
      <RenameDialog
        description={t('sidebar.renameConversationDescription')}
        initialValue={task.title}
        label={t('sidebar.conversationName')}
        onOpenChange={setRenameOpen}
        onSave={(title) => onRenameTask(task, title)}
        open={renameOpen}
        title={t('sidebar.renameConversation')}
      />
      <ConfirmDialog
        confirmLabel={t('sidebar.archiveConversation')}
        description={t('sidebar.archiveConversationDescription')}
        onConfirm={() => onSetTaskArchived(task, true)}
        onOpenChange={setArchiveOpen}
        open={archiveOpen}
        title={t('sidebar.archiveConversationTitle', { name: task.title })}
      />
      <ConfirmDialog
        confirmLabel={t('sidebar.removeConversation')}
        description={t('sidebar.removeConversationDescription')}
        onConfirm={() => onRemoveTask(task)}
        onOpenChange={setRemoveOpen}
        open={removeOpen}
        title={t('sidebar.removeConversationTitle', { name: task.title })}
      />
    </li>
  )
}

function RenameDialog({
  description,
  initialValue,
  label,
  onOpenChange,
  onSave,
  open,
  title,
}: {
  description: string
  initialValue: string
  label: string
  onOpenChange: (open: boolean) => void
  onSave: (value: string) => MaybePromise
  open: boolean
  title: string
}) {
  const { t } = useTranslation('shell')
  const [value, setValue] = useState(initialValue)
  const trimmed = value.trim()

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!trimmed) return
    void onSave(trimmed)
    onOpenChange(false)
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (nextOpen) setValue(initialValue)
        onOpenChange(nextOpen)
      }}
      open={open}
    >
      <DialogContent>
        <form className="grid gap-4" onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>{description}</DialogDescription>
          </DialogHeader>
          <label className="grid gap-1.5 font-medium text-sm">
            <span>{label}</span>
            <input
              aria-label={label}
              autoFocus
              className="h-9 rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              maxLength={120}
              onChange={(event) => setValue(event.target.value)}
              value={value}
            />
          </label>
          <DialogFooter>
            <Button onClick={() => onOpenChange(false)} type="button" variant="ghost">
              {t('sidebar.cancel')}
            </Button>
            <Button disabled={!trimmed} type="submit">
              {t('sidebar.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function ConfirmDialog({
  confirmLabel,
  description,
  onConfirm,
  onOpenChange,
  open,
  title,
}: {
  confirmLabel: string
  description: string
  onConfirm: () => MaybePromise
  onOpenChange: (open: boolean) => void
  open: boolean
  title: string
}) {
  const { t } = useTranslation('shell')
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)} type="button" variant="ghost">
            {t('sidebar.cancel')}
          </Button>
          <Button
            onClick={() => {
              void onConfirm()
              onOpenChange(false)
            }}
            type="button"
            variant="destructive"
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function EmptyRow({ children, compact }: { children: React.ReactNode; compact: boolean }) {
  return compact ? null : <p className="px-6 py-1.5 text-muted-foreground text-xs">{children}</p>
}

type DisplayStatus =
  | 'completed'
  | 'failed'
  | 'idle'
  | 'interrupted'
  | 'queued'
  | 'running'
  | 'waiting'

function taskStatus(
  task: TaskProjection,
  t: TFunction<'shell'>,
): { key: DisplayStatus; label: string } {
  if (task.state === 'running' || task.state === 'yielding') {
    return {
      key: 'running',
      label: t(task.state === 'yielding' ? 'sidebar.status.yielding' : 'sidebar.status.running'),
    }
  }
  if (task.state === 'waiting_permission') {
    return { key: 'waiting', label: t('sidebar.status.waitingPermission') }
  }
  if (task.state === 'waiting_input') {
    return { key: 'waiting', label: t('sidebar.status.waitingInput') }
  }
  if (task.queue.some((item) => item.state === 'queued' || item.state === 'promoting')) {
    const count = task.queue.filter(
      (item) => item.state === 'queued' || item.state === 'promoting',
    ).length
    return { key: 'queued', label: t('sidebar.status.queued', { count }) }
  }
  if (task.state === 'interrupted') {
    return { key: 'interrupted', label: t('sidebar.status.interrupted') }
  }
  if (task.state === 'failed') return { key: 'failed', label: t('sidebar.status.failed') }
  return {
    key: task.state === 'idle' ? 'idle' : 'completed',
    label: t(task.state === 'idle' ? 'sidebar.status.ready' : 'sidebar.status.completed'),
  }
}

function TaskStatusIcon({ status }: { status: DisplayStatus }) {
  const className = cn('size-3.5 shrink-0', statusColor(status))
  if (status === 'running')
    return <LoaderCircle aria-hidden="true" className={`${className} animate-spin`} />
  if (status === 'waiting') return <CirclePause aria-hidden="true" className={className} />
  if (status === 'queued') return <MessageSquareMore aria-hidden="true" className={className} />
  if (status === 'interrupted') return <CircleDashed aria-hidden="true" className={className} />
  if (status === 'failed') return <CircleAlert aria-hidden="true" className={className} />
  return <CircleCheck aria-hidden="true" className={className} />
}

function statusColor(status: DisplayStatus) {
  if (status === 'running') return 'text-state-running'
  if (status === 'waiting') return 'text-state-waiting'
  if (status === 'queued') return 'text-state-queued'
  if (status === 'interrupted') return 'text-state-interrupted'
  if (status === 'failed') return 'text-state-failed'
  if (status === 'idle') return 'text-state-idle'
  return 'text-state-completed'
}
