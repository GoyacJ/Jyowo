import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { motion } from 'framer-motion'
import {
  ChevronDown,
  ChevronRight,
  FolderPlus,
  Loader2,
  MoreHorizontal,
  MoveDown,
  MoveUp,
  NotebookText,
  PanelLeftClose,
  PanelLeftOpen,
  Pin,
  SquarePen,
  Trash2,
} from 'lucide-react'
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { conversationQueryKeys } from '@/features/conversation/use-conversation'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import {
  addProject,
  createDefaultConversation as createDefaultConversationCommand,
  createProjectConversation as createProjectConversationCommand,
  type DeleteProjectResponse,
  deleteConversation as deleteConversationCommand,
  deleteProject as deleteProjectCommand,
  deleteProjectConversation as deleteProjectConversationCommand,
  type ListConversationsResponse,
  type ListProjectConversationGroupsResponse,
  type ListProjectsResponse,
  listConversations,
  listProjectConversationGroups,
  moveProject as moveProjectCommand,
  switchProject as switchProjectCommand,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickProjectDirectory } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
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
import { ScrollArea } from '@/shared/ui/scroll-area'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { onProjectWorkspaceChanged } from './reset-workspace-scope'
import { useActiveProjectPath } from './use-active-project-path'

type SidebarNavProps = {
  compact?: boolean
}

type ProjectConversationGroup = ListProjectConversationGroupsResponse['groups'][number]
type ProjectConversation = ProjectConversationGroup['conversations'][number]
type ProjectRecord = ProjectConversationGroup['project']
type PinnedProjectConversation = {
  conversation: ProjectConversation
  projectPath: string
}

const PROJECT_CONVERSATION_GROUPS_QUERY_KEY = ['project-conversation-groups'] as const
const PINNED_CONVERSATION_IDS_STORAGE_KEY = 'jyowo.sidebar.pinnedConversationIds'
type SidebarSectionKey = 'conversations' | 'pinned' | 'projects'
const DEFAULT_EXPANDED_SECTIONS: SidebarSectionKey[] = ['pinned', 'projects', 'conversations']

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const { t } = useTranslation(['shell', 'conversation'])
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const selectedConversationId = useRouterState({
    select: (state) => state.location.search.conversationId,
  })
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const activeRunsByConversation = useUiStore((state) => state.activeRunsByConversation)
  const [expandedProjectPaths, setExpandedProjectPaths] = useState(() => new Set<string>())
  const initializedExpandedProjectPathsRef = useRef(new Set<string>())
  const [expandedSections, setExpandedSections] = useState(
    () => new Set<SidebarSectionKey>(DEFAULT_EXPANDED_SECTIONS),
  )
  const [pinnedConversationIds, setPinnedConversationIds] = useState(readPinnedConversationIds)
  const [createdDefaultConversations, setCreatedDefaultConversations] = useState<
    ProjectConversation[]
  >([])
  const [pendingDeleteProject, setPendingDeleteProject] = useState<ProjectRecord | null>(null)
  const [navigationError, setNavigationError] = useState<unknown>(null)
  const activeProjectPathQuery = useActiveProjectPath()
  const workspacePath = activeProjectPathQuery.data ?? null
  const projectConversationGroupsQuery = useQuery({
    queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
    queryFn: () => listProjectConversationGroups(commandClient),
  })
  const projectGroups = projectConversationGroupsQuery.data?.groups ?? []
  const hasProjectGroups = projectGroups.length > 0
  const activeProjectPath = projectConversationGroupsQuery.data
    ? projectConversationGroupsQuery.data.activePath
    : workspacePath
  const workspaceKey = activeProjectPath ?? 'none'
  const shouldLoadDefaultConversations =
    projectConversationGroupsQuery.isSuccess && (!hasProjectGroups || activeProjectPath === null)
  const conversationsQuery = useQuery({
    enabled: shouldLoadDefaultConversations,
    queryKey: conversationQueryKeys.list(workspaceKey),
    queryFn: () => listConversations(commandClient),
  })
  const createDefaultConversationMutation = useMutation({
    mutationFn: () => createDefaultConversationCommand(commandClient),
    onSuccess: async (response) => {
      queryClient.setQueryData<ListConversationsResponse>(
        conversationQueryKeys.list('none'),
        (current) => {
          if (!current) {
            return { conversations: [response.conversation] }
          }

          return {
            conversations: [
              response.conversation,
              ...current.conversations.filter(
                (conversation) => conversation.id !== response.conversation.id,
              ),
            ],
          }
        },
      )
      queryClient.setQueryData<ListProjectsResponse>(['projects', 'list'], (current) =>
        current ? { ...current, activePath: null } : current,
      )
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => (current ? { ...current, activePath: null } : current),
      )
      setCreatedDefaultConversations((current) =>
        mergeCreatedConversationsWithFetched([response.conversation], current),
      )
      setExpandedSections((current) => new Set(current).add('conversations'))
      void navigate({ search: { conversationId: response.conversation.id }, to: '/' }).then(() => {
        window.setTimeout(() => {
          document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
        }, 0)
      })
      void queryClient.invalidateQueries({ queryKey: ['projects', 'list'] })
    },
  })
  const createProjectConversationMutation = useMutation({
    mutationFn: (projectPath: string) =>
      createProjectConversationCommand(projectPath, commandClient),
    onSuccess: async (response, projectPath) => {
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => addConversationToProjectGroup(current, projectPath, response.conversation),
      )
      setExpandedProjectPaths((current) => new Set(current).add(projectPath))
      if (projectPath !== activeProjectPath) {
        await switchProjectCommand(projectPath, commandClient)
        await onProjectWorkspaceChanged(queryClient, navigate)
      }
      void navigate({ search: { conversationId: response.conversation.id }, to: '/' }).then(() => {
        window.setTimeout(() => {
          document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
        }, 0)
      })
      void queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
    },
    onError: (error) => setNavigationError(error),
  })
  const deleteConversationMutation = useMutation({
    mutationFn: (conversationId: string) =>
      deleteConversationCommand(conversationId, commandClient),
    onSuccess: async (_, conversationId) => {
      clearActiveRun(conversationId)
      setPinnedConversationIds((current) => removePinnedConversationId(current, conversationId))
      setCreatedDefaultConversations((current) =>
        current.filter((conversation) => conversation.id !== conversationId),
      )
      queryClient.setQueryData<ListConversationsResponse>(
        conversationQueryKeys.list(workspaceKey),
        (current) => removeConversationFromList(current, conversationId),
      )
      queryClient.setQueryData<ListConversationsResponse>(
        conversationQueryKeys.list('none'),
        (current) => removeConversationFromList(current, conversationId),
      )
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => removeConversationFromProjectGroups(current, conversationId),
      )
      await queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })

      if (selectedConversationId === conversationId) {
        void navigate({ to: '/' })
      }
    },
  })
  const deleteProjectConversationMutation = useMutation({
    mutationFn: ({
      conversationId,
      projectPath,
    }: {
      conversationId: string
      projectPath: string
    }) => deleteProjectConversationCommand(projectPath, conversationId, commandClient),
    onSuccess: async (_, { conversationId }) => {
      clearActiveRun(conversationId)
      setPinnedConversationIds((current) => removePinnedConversationId(current, conversationId))
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => removeConversationFromProjectGroups(current, conversationId),
      )
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })

      if (selectedConversationId === conversationId) {
        void navigate({ to: '/' })
      }
    },
  })
  const moveProjectMutation = useMutation({
    mutationFn: ({ direction, path }: { direction: 'up' | 'down'; path: string }) =>
      moveProjectCommand(path, direction, commandClient),
    onSuccess: async (response) => {
      queryClient.setQueryData(['projects', 'list'], response)
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
    },
  })
  const addProjectMutation = useMutation({
    mutationFn: (path: string) => addProject(path, commandClient),
    onSuccess: async () => {
      await onProjectWorkspaceChanged(queryClient, navigate)
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
    },
  })
  const deleteProjectMutation = useMutation({
    mutationFn: (path: string) => deleteProjectCommand(path, commandClient),
    onSuccess: async (response) => {
      removeDeletedProjectFromCache(queryClient, response)
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
      await queryClient.invalidateQueries({ queryKey: ['projects', 'list'] })
      if (response.activePath === null) {
        await onProjectWorkspaceChanged(queryClient, navigate)
      }
    },
    onSettled: () => setPendingDeleteProject(null),
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const conversationListError =
    createDefaultConversationMutation.error ??
    createProjectConversationMutation.error ??
    deleteConversationMutation.error ??
    deleteProjectConversationMutation.error ??
    addProjectMutation.error ??
    deleteProjectMutation.error ??
    moveProjectMutation.error ??
    navigationError ??
    projectConversationGroupsQuery.error ??
    conversationsQuery.error
  const conversationListErrorMessage = conversationListError
    ? getCommandErrorMessage(conversationListError)
    : undefined
  const pinnedConversations = useMemo(
    () => getPinnedProjectConversations(projectGroups, pinnedConversationIds),
    [pinnedConversationIds, projectGroups],
  )
  const visibleProjectGroups = useMemo(
    () => removePinnedConversationsFromGroups(projectGroups, pinnedConversationIds),
    [pinnedConversationIds, projectGroups],
  )
  const runningConversationIds = useMemo(
    () => new Set(Object.keys(activeRunsByConversation)),
    [activeRunsByConversation],
  )
  const runningProjectPaths = useMemo(
    () =>
      new Set(
        projectGroups
          .filter((group) => conversationsHaveRunning(group.conversations, runningConversationIds))
          .map((group) => group.project.path),
      ),
    [projectGroups, runningConversationIds],
  )
  const defaultConversations = useMemo(() => {
    if (activeProjectPath !== null) {
      return []
    }

    const fetchedDefaultConversations = conversationsQuery.data?.conversations ?? []

    return mergeCreatedConversationsWithFetched(
      createdDefaultConversations,
      fetchedDefaultConversations,
    )
  }, [activeProjectPath, conversationsQuery.data?.conversations, createdDefaultConversations])

  useEffect(() => {
    if (projectGroups.length === 0) {
      return
    }

    setExpandedProjectPaths((current) => {
      let next = current
      for (const group of projectGroups) {
        if (initializedExpandedProjectPathsRef.current.has(group.project.path)) {
          continue
        }
        initializedExpandedProjectPathsRef.current.add(group.project.path)
        if (!next.has(group.project.path)) {
          next = new Set(next)
          next.add(group.project.path)
        }
      }
      return next
    })
  }, [projectGroups])

  useEffect(() => {
    writePinnedConversationIds(pinnedConversationIds)
  }, [pinnedConversationIds])

  function selectConversation(conversationId: string) {
    void navigate({ search: { conversationId }, to: '/' })
  }

  async function selectProjectConversation(projectPath: string, conversationId: string) {
    try {
      setNavigationError(null)
      if (projectPath !== activeProjectPath) {
        await switchProjectCommand(projectPath, commandClient)
        await onProjectWorkspaceChanged(queryClient, navigate)
      }
      void navigate({ search: { conversationId }, to: '/' })
    } catch (error) {
      setNavigationError(error)
    }
  }

  function focusComposerForNewConversation() {
    createDefaultConversationMutation.mutate()
  }

  function createConversationInProject(projectPath: string) {
    setNavigationError(null)
    createProjectConversationMutation.mutate(projectPath)
  }

  function deleteConversation(conversationId: string) {
    deleteConversationMutation.mutate(conversationId)
  }

  function deleteProjectConversation(projectPath: string, conversationId: string) {
    deleteProjectConversationMutation.mutate({ conversationId, projectPath })
  }

  function togglePinnedConversation(conversationId: string) {
    setPinnedConversationIds((current) => {
      const next = new Set(current)
      if (next.has(conversationId)) {
        next.delete(conversationId)
      } else {
        next.add(conversationId)
      }
      return next
    })
  }

  async function openProjectDirectory() {
    const selectedPath = await pickProjectDirectory()
    if (!selectedPath) {
      return
    }

    addProjectMutation.mutate(selectedPath)
  }

  function toggleProjectExpanded(projectPath: string) {
    setExpandedProjectPaths((current) => {
      const next = new Set(current)
      if (next.has(projectPath)) {
        next.delete(projectPath)
      } else {
        next.add(projectPath)
      }
      return next
    })
  }

  function toggleSection(section: SidebarSectionKey) {
    setExpandedSections((current) => {
      const next = new Set(current)
      if (next.has(section)) {
        next.delete(section)
      } else {
        next.add(section)
      }
      return next
    })
  }

  function runCommand(action: CommandPaletteAction) {
    if (action === 'new-conversation') {
      focusComposerForNewConversation()
      return
    }

    if (action === 'open-evals') {
      void navigate({ to: '/evals' })
      return
    }

    if (action === 'settings') {
      setInspectorOpen(true)
      void navigate({ to: '/settings' })
    }
  }

  function moveProject(path: string, direction: 'up' | 'down') {
    moveProjectMutation.mutate({ direction, path })
  }

  function confirmDeleteProject() {
    if (!pendingDeleteProject) {
      return
    }

    deleteProjectMutation.mutate(pendingDeleteProject.path)
  }

  if (sidebarCollapsed || compact) {
    return (
      <nav
        aria-label={t('workspace')}
        className="flex min-h-0 flex-col items-center border-border border-r bg-muted/45 py-3"
        data-collapsed="true"
      >
        <CommandPalette onAction={runCommand} />
        <button
          aria-label={t('actions.newConversation')}
          className="mt-2 grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={focusComposerForNewConversation}
          title={t('actions.newConversation')}
          type="button"
        >
          <SquarePen className="size-4" />
        </button>
        <button
          aria-label={t('projects.new')}
          className="mt-2 grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => void openProjectDirectory()}
          title={t('projects.new')}
          type="button"
        >
          <FolderPlus className="size-4" />
        </button>
        <button
          aria-label={t('actions.expandSidebar')}
          className="mt-3 grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(false)}
          title={t('actions.expandSidebar')}
          type="button"
        >
          <PanelLeftOpen className="size-4" />
        </button>
      </nav>
    )
  }

  return (
    <nav
      aria-label={t('workspace')}
      className="flex min-h-0 flex-col border-border border-r bg-muted/55"
      data-collapsed="false"
    >
      <CommandPalette onAction={runCommand} />
      <div className="flex shrink-0 items-center gap-1 px-3 pt-2">
        <button
          className="flex h-8 min-w-0 flex-1 items-center gap-3 rounded-md px-2 text-left font-medium text-foreground/85 text-sm hover:bg-background/55 hover:text-foreground"
          onClick={focusComposerForNewConversation}
          type="button"
        >
          <SquarePen aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate">{t('actions.newConversation')}</span>
        </button>
        <button
          aria-label={t('actions.collapseSidebar')}
          className="grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground/80 hover:bg-background/55 hover:text-foreground"
          onClick={() => setSidebarCollapsed(true)}
          type="button"
        >
          <PanelLeftClose className="size-4" />
        </button>
      </div>
      {projectConversationGroupsQuery.isLoading || projectConversationGroupsQuery.isSuccess ? (
        <>
          <ProjectConversationGroups
            activeConversationId={selectedConversationId}
            activeProjectPath={activeProjectPath}
            defaultConversations={defaultConversations}
            errorMessage={conversationListErrorMessage}
            expandedProjectPaths={expandedProjectPaths}
            expandedSections={expandedSections}
            groups={visibleProjectGroups}
            isDefaultConversationsLoading={
              activeProjectPath === null && conversationsQuery.isLoading
            }
            isLoading={projectConversationGroupsQuery.isLoading}
            onDeleteConversation={deleteConversation}
            onDeleteProjectConversation={deleteProjectConversation}
            onMoveProject={moveProject}
            onNewConversation={(projectPath) => {
              createConversationInProject(projectPath)
            }}
            onNewProject={() => void openProjectDirectory()}
            onPinConversation={togglePinnedConversation}
            onRemoveProject={setPendingDeleteProject}
            onSelectDefaultConversation={selectConversation}
            onSelectConversation={(projectPath, conversationId) => {
              void selectProjectConversation(projectPath, conversationId)
            }}
            onToggleProjectExpanded={toggleProjectExpanded}
            onToggleSection={toggleSection}
            pinnedConversationIds={pinnedConversationIds}
            pinnedConversations={pinnedConversations}
            runningConversationIds={runningConversationIds}
            runningProjectPaths={runningProjectPaths}
          />
          <Dialog
            onOpenChange={(open) => {
              if (!open) {
                setPendingDeleteProject(null)
              }
            }}
            open={Boolean(pendingDeleteProject)}
          >
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t('projects.confirmDeleteTitle')}</DialogTitle>
                <DialogDescription>
                  {t('projects.confirmDeleteDescription', {
                    name: pendingDeleteProject?.name ?? '',
                  })}
                </DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <Button
                  onClick={() => setPendingDeleteProject(null)}
                  type="button"
                  variant="outline"
                >
                  {t('actions.cancel')}
                </Button>
                <Button onClick={confirmDeleteProject} type="button" variant="destructive">
                  {t('projects.confirmDelete')}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </>
      ) : (
        <div className="mt-5 shrink-0 rounded-md px-5 py-2 text-destructive text-xs">
          {conversationListErrorMessage}
        </div>
      )}
    </nav>
  )
}

function SidebarSectionHeader({
  action,
  children,
  isExpanded,
  isRunning,
  onToggle,
}: {
  action?: ReactNode
  children: string
  isExpanded: boolean
  isRunning: boolean
  onToggle: () => void
}) {
  const { t } = useTranslation('shell')

  return (
    <div className="flex h-7 items-center gap-1">
      <button
        aria-label={
          isExpanded
            ? t('sections.collapse', { name: children })
            : t('sections.expand', { name: children })
        }
        className="flex h-full min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 font-medium text-muted-foreground/75 text-xs uppercase tracking-normal hover:bg-background/45 hover:text-foreground"
        onClick={onToggle}
        type="button"
      >
        {isExpanded ? (
          <ChevronDown aria-hidden="true" className="size-3.5 shrink-0" />
        ) : (
          <ChevronRight aria-hidden="true" className="size-3.5 shrink-0" />
        )}
        <span className="min-w-0 truncate">{children}</span>
      </button>
      {isRunning ? <RunningIndicator label={t('sections.running', { name: children })} /> : null}
      {action}
    </div>
  )
}

function RunningIndicator({ label }: { label: string }) {
  return (
    <span
      aria-label={label}
      className="grid size-5 shrink-0 place-items-center text-warning"
      role="status"
    >
      <Loader2 aria-hidden="true" className="size-3.5 animate-spin" strokeWidth={1.9} />
    </span>
  )
}

function ProjectHeaderRow({
  isActive,
  isExpanded,
  onCreateConversation,
  onMoveDown,
  onMoveUp,
  onRemoveProject,
  onToggle,
  project,
  projectName,
  isRunning,
}: {
  isActive: boolean
  isExpanded: boolean
  isRunning: boolean
  onCreateConversation: () => void
  onMoveDown: () => void
  onMoveUp: () => void
  onRemoveProject: (project: ProjectRecord) => void
  onToggle: () => void
  project: ProjectRecord
  projectName: string
}) {
  const { t } = useTranslation('shell')

  return (
    <div
      className={cn(
        'group relative grid h-8 min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-1 rounded-md pr-1 text-muted-foreground hover:bg-background/45 hover:text-foreground data-[active=true]:text-foreground',
        isActive && 'text-foreground',
      )}
      data-active={isActive}
      data-depth="project"
      data-sidebar-row="true"
    >
      {isActive && (
        <motion.div
          layoutId="activeSidebarIndicator"
          className="absolute inset-0 bg-background/55 rounded-md -z-10 shadow-[inset_0_1px_0_rgba(255,255,255,0.05),0_1px_2px_rgba(0,0,0,0.05)]"
          transition={{ type: 'spring', stiffness: 380, damping: 30 }}
        />
      )}
      <button
        aria-label={
          isExpanded
            ? t('projects.collapseGroup', { name: projectName })
            : t('projects.expandGroup', { name: projectName })
        }
        className="flex h-full min-w-0 items-center gap-2 overflow-hidden rounded-md px-2 text-left"
        onClick={onToggle}
        type="button"
      >
        <NotebookText aria-hidden="true" className="size-4 shrink-0" strokeWidth={1.7} />
        <span className="min-w-0 truncate font-semibold text-sm">{projectName}</span>
        {isExpanded ? (
          <ChevronDown aria-hidden="true" className="size-3.5 shrink-0 opacity-70" />
        ) : (
          <ChevronRight aria-hidden="true" className="size-3.5 shrink-0 opacity-70" />
        )}
      </button>
      {isRunning ? <RunningIndicator label={t('sections.running', { name: projectName })} /> : null}
      <div
        className={cn(
          'pointer-events-none absolute top-1/2 right-1 flex -translate-y-1/2 items-center gap-1 opacity-0 transition-opacity group-hover:pointer-events-auto group-hover:opacity-100 focus-within:pointer-events-auto focus-within:opacity-100',
          isRunning && 'right-7',
        )}
      >
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              aria-label={t('projects.actions', { name: projectName })}
              className="grid size-7 place-items-center rounded-md hover:bg-background data-[state=open]:bg-background"
              type="button"
            >
              <MoreHorizontal className="size-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onSelect={onMoveUp}>
              <MoveUp aria-hidden="true" className="size-4 text-muted-foreground" />
              {t('projects.moveUp')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onMoveDown}>
              <MoveDown aria-hidden="true" className="size-4 text-muted-foreground" />
              {t('projects.moveDown')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-destructive focus:text-destructive"
              onSelect={() => onRemoveProject(project)}
            >
              <Trash2 aria-hidden="true" className="size-4" />
              {t('projects.deleteShort')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <button
          aria-label={t('projects.newConversation', { name: projectName })}
          className="grid size-7 place-items-center rounded-md hover:bg-background"
          onClick={onCreateConversation}
          type="button"
        >
          <SquarePen className="size-4" />
        </button>
      </div>
    </div>
  )
}

function ProjectConversationGroups({
  activeConversationId,
  activeProjectPath,
  defaultConversations,
  errorMessage,
  expandedProjectPaths,
  expandedSections,
  groups,
  isDefaultConversationsLoading,
  isLoading,
  onDeleteConversation,
  onDeleteProjectConversation,
  onMoveProject,
  onNewConversation,
  onNewProject,
  onPinConversation,
  onRemoveProject,
  onSelectDefaultConversation,
  onSelectConversation,
  onToggleProjectExpanded,
  onToggleSection,
  pinnedConversationIds,
  pinnedConversations,
  runningConversationIds,
  runningProjectPaths,
}: {
  activeConversationId?: string
  activeProjectPath: string | null
  defaultConversations: ProjectConversation[]
  errorMessage?: string
  expandedProjectPaths: Set<string>
  expandedSections: Set<SidebarSectionKey>
  groups: ProjectConversationGroup[]
  isDefaultConversationsLoading: boolean
  isLoading: boolean
  onDeleteConversation: (conversationId: string) => void
  onDeleteProjectConversation: (projectPath: string, conversationId: string) => void
  onMoveProject: (projectPath: string, direction: 'up' | 'down') => void
  onNewConversation: (projectPath: string) => void
  onNewProject: () => void
  onPinConversation: (conversationId: string) => void
  onRemoveProject: (project: ProjectRecord) => void
  onSelectDefaultConversation: (conversationId: string) => void
  onSelectConversation: (projectPath: string, conversationId: string) => void
  onToggleProjectExpanded: (projectPath: string) => void
  onToggleSection: (section: SidebarSectionKey) => void
  pinnedConversationIds: Set<string>
  pinnedConversations: PinnedProjectConversation[]
  runningConversationIds: Set<string>
  runningProjectPaths: Set<string>
}) {
  const { t } = useTranslation('shell')
  const pinnedExpanded = expandedSections.has('pinned')
  const projectsExpanded = expandedSections.has('projects')
  const conversationsExpanded = expandedSections.has('conversations')
  const pinnedHasRunning = conversationsHaveRunning(
    pinnedConversations.map(({ conversation }) => conversation),
    runningConversationIds,
  )
  const projectsHaveRunning = runningProjectPaths.size > 0
  const defaultConversationsHaveRunning = conversationsHaveRunning(
    defaultConversations,
    runningConversationIds,
  )

  return (
    <div className="mt-5 flex min-h-0 flex-1 flex-col px-3">
      {isLoading ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.loading')}
        </div>
      ) : null}
      {!isLoading && errorMessage ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-destructive text-xs">{errorMessage}</div>
      ) : null}
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 pr-1 pb-4">
          <section>
            <SidebarSectionHeader
              isExpanded={pinnedExpanded}
              isRunning={!pinnedExpanded && pinnedHasRunning}
              onToggle={() => onToggleSection('pinned')}
            >
              {t('sections.pinned')}
            </SidebarSectionHeader>
            {pinnedExpanded && pinnedConversations.length ? (
              <ul className="flex flex-col gap-0.5">
                {pinnedConversations.map(({ conversation, projectPath }) => (
                  <ProjectConversationRow
                    activeConversationId={activeConversationId}
                    activeProjectPath={activeProjectPath}
                    conversation={conversation}
                    isPinned={pinnedConversationIds.has(conversation.id)}
                    isRunning={runningConversationIds.has(conversation.id)}
                    key={conversation.id}
                    onDeleteConversation={onDeleteConversation}
                    onDeleteProjectConversation={onDeleteProjectConversation}
                    onPinConversation={onPinConversation}
                    onSelectConversation={onSelectConversation}
                    projectPath={projectPath}
                  />
                ))}
              </ul>
            ) : null}
          </section>
          <section>
            <SidebarSectionHeader
              action={
                <button
                  aria-label={t('projects.new')}
                  className="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-background/55 hover:text-foreground"
                  onClick={onNewProject}
                  title={t('projects.new')}
                  type="button"
                >
                  <FolderPlus className="size-3.5" />
                </button>
              }
              isExpanded={projectsExpanded}
              isRunning={!projectsExpanded && projectsHaveRunning}
              onToggle={() => onToggleSection('projects')}
            >
              {t('sections.projects')}
            </SidebarSectionHeader>
            {projectsExpanded
              ? groups.map((group) => {
                  const isProjectActive = group.project.path === activeProjectPath
                  const isExpanded = expandedProjectPaths.has(group.project.path)
                  const visibleConversations = isExpanded ? group.conversations : []
                  const groupHasRunning = runningProjectPaths.has(group.project.path)

                  return (
                    <section data-active={isProjectActive} key={group.project.path}>
                      <ProjectHeaderRow
                        isActive={isProjectActive}
                        isExpanded={isExpanded}
                        isRunning={!isExpanded && groupHasRunning}
                        onCreateConversation={() => onNewConversation(group.project.path)}
                        onMoveDown={() => onMoveProject(group.project.path, 'down')}
                        onMoveUp={() => onMoveProject(group.project.path, 'up')}
                        onRemoveProject={onRemoveProject}
                        onToggle={() => onToggleProjectExpanded(group.project.path)}
                        project={group.project}
                        projectName={group.project.name}
                      />
                      {visibleConversations.length ? (
                        <ul className="mt-1 flex flex-col gap-0.5">
                          {visibleConversations.map((conversation) => (
                            <ProjectConversationRow
                              activeConversationId={activeConversationId}
                              activeProjectPath={activeProjectPath}
                              conversation={conversation}
                              isPinned={pinnedConversationIds.has(conversation.id)}
                              isRunning={runningConversationIds.has(conversation.id)}
                              key={conversation.id}
                              onDeleteConversation={onDeleteConversation}
                              onDeleteProjectConversation={onDeleteProjectConversation}
                              onPinConversation={onPinConversation}
                              onSelectConversation={onSelectConversation}
                              projectPath={group.project.path}
                            />
                          ))}
                        </ul>
                      ) : null}
                    </section>
                  )
                })
              : null}
          </section>
          <section>
            <SidebarSectionHeader
              isExpanded={conversationsExpanded}
              isRunning={!conversationsExpanded && defaultConversationsHaveRunning}
              onToggle={() => onToggleSection('conversations')}
            >
              {t('sections.conversations')}
            </SidebarSectionHeader>
            {conversationsExpanded && isDefaultConversationsLoading ? (
              <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
                {t('conversations.loading')}
              </div>
            ) : null}
            {conversationsExpanded &&
            !isDefaultConversationsLoading &&
            defaultConversations.length === 0 ? (
              <div className="shrink-0 rounded-md px-6 py-1.5 text-muted-foreground text-xs">
                {t('conversations.empty')}
              </div>
            ) : null}
            {conversationsExpanded && defaultConversations.length > 0 ? (
              <ul className="flex flex-col gap-0.5">
                {defaultConversations.map((conversation) => (
                  <DefaultConversationRow
                    activeConversationId={activeConversationId}
                    conversation={conversation}
                    isRunning={runningConversationIds.has(conversation.id)}
                    key={conversation.id}
                    onDeleteConversation={onDeleteConversation}
                    onSelectConversation={onSelectDefaultConversation}
                  />
                ))}
              </ul>
            ) : null}
          </section>
        </div>
      </ScrollArea>
    </div>
  )
}

function ProjectConversationRow({
  activeConversationId,
  activeProjectPath,
  conversation,
  isPinned,
  isRunning,
  onDeleteConversation,
  onDeleteProjectConversation,
  onPinConversation,
  onSelectConversation,
  projectPath,
}: {
  activeConversationId?: string
  activeProjectPath: string | null
  conversation: ProjectConversation
  isPinned: boolean
  isRunning: boolean
  onDeleteConversation: (conversationId: string) => void
  onDeleteProjectConversation: (projectPath: string, conversationId: string) => void
  onPinConversation: (conversationId: string) => void
  onSelectConversation: (projectPath: string, conversationId: string) => void
  projectPath: string
}) {
  const { t } = useTranslation('shell')
  const isProjectActive = projectPath === activeProjectPath
  const isActive = isProjectActive && conversation.id === activeConversationId
  const title = conversation.isEmpty ? t('conversations.defaultTitle') : conversation.title
  const relativeTime = formatSidebarRelativeTime(conversation.updatedAt, t)

  return (
    <li>
      <div
        className="group relative grid h-8 w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-1 rounded-md pr-1 text-muted-foreground hover:bg-background/45 hover:text-foreground data-[active=true]:text-foreground"
        data-active={isActive}
        data-depth="conversation"
        data-sidebar-row="true"
      >
        {isActive && (
          <motion.div
            layoutId="activeSidebarIndicator"
            className="absolute inset-0 bg-background/55 rounded-md -z-10 shadow-[inset_0_1px_0_rgba(255,255,255,0.05),0_1px_2px_rgba(0,0,0,0.05)]"
            transition={{ type: 'spring', stiffness: 380, damping: 30 }}
          />
        )}
        <button
          aria-current={isActive ? 'page' : undefined}
          className="grid h-full min-w-0 grid-cols-[minmax(0,1fr)_3.5rem] items-center gap-2 overflow-hidden rounded-md py-1 pr-2 pl-9 text-left text-xs"
          onClick={() => onSelectConversation(projectPath, conversation.id)}
          type="button"
        >
          <span className="block min-w-0 flex-1 truncate font-medium">{title}</span>
          <span className="truncate text-right text-muted-foreground/80 text-xs">
            {relativeTime}
          </span>
        </button>
        {isRunning ? <RunningIndicator label={t('conversations.running', { title })} /> : null}
        <div
          className={cn(
            'pointer-events-none absolute top-1/2 right-1 flex -translate-y-1/2 items-center gap-1 group-hover:pointer-events-auto focus-within:pointer-events-auto',
            isRunning && 'right-7',
          )}
        >
          <button
            aria-label={
              isPinned ? t('conversations.unpin', { title }) : t('conversations.pin', { title })
            }
            className={cn(
              'grid size-6 place-items-center rounded-md bg-muted/80 text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100',
              isPinned && 'opacity-100 text-foreground',
            )}
            onClick={() => onPinConversation(conversation.id)}
            title={
              isPinned ? t('conversations.unpin', { title }) : t('conversations.pin', { title })
            }
            type="button"
          >
            <Pin
              className="size-3.5"
              fill={isPinned ? 'currentColor' : 'none'}
              strokeWidth={1.75}
            />
          </button>
          <button
            aria-label={t('conversations.delete', { title })}
            className="grid size-6 place-items-center rounded-md bg-muted/80 text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
            onClick={() => {
              if (isProjectActive) {
                onDeleteConversation(conversation.id)
                return
              }
              onDeleteProjectConversation(projectPath, conversation.id)
            }}
            title={t('conversations.delete', { title })}
            type="button"
          >
            <Trash2 className="size-3.5" strokeWidth={1.75} />
          </button>
        </div>
      </div>
    </li>
  )
}

function DefaultConversationRow({
  activeConversationId,
  conversation,
  isRunning,
  onDeleteConversation,
  onSelectConversation,
}: {
  activeConversationId?: string
  conversation: ProjectConversation
  isRunning: boolean
  onDeleteConversation: (conversationId: string) => void
  onSelectConversation: (conversationId: string) => void
}) {
  const { t } = useTranslation('shell')
  const isActive = conversation.id === activeConversationId
  const title = conversation.isEmpty ? t('conversations.defaultTitle') : conversation.title
  const relativeTime = formatSidebarRelativeTime(conversation.updatedAt, t)

  return (
    <li>
      <div
        className="group relative grid h-8 w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-1 rounded-md pr-1 text-muted-foreground hover:bg-background/45 hover:text-foreground data-[active=true]:text-foreground"
        data-active={isActive}
        data-depth="conversation"
        data-sidebar-row="true"
      >
        {isActive && (
          <motion.div
            layoutId="activeSidebarIndicator"
            className="absolute inset-0 bg-background/55 rounded-md -z-10 shadow-[inset_0_1px_0_rgba(255,255,255,0.05),0_1px_2px_rgba(0,0,0,0.05)]"
            transition={{ type: 'spring', stiffness: 380, damping: 30 }}
          />
        )}
        <button
          aria-current={isActive ? 'page' : undefined}
          className="grid h-full min-w-0 grid-cols-[minmax(0,1fr)_3.5rem] items-center gap-2 overflow-hidden rounded-md py-1 pr-2 pl-5 text-left text-xs"
          onClick={() => onSelectConversation(conversation.id)}
          type="button"
        >
          <span className="block min-w-0 flex-1 truncate font-medium">{title}</span>
          <span className="truncate text-right text-muted-foreground/80 text-xs">
            {relativeTime}
          </span>
        </button>
        {isRunning ? <RunningIndicator label={t('conversations.running', { title })} /> : null}
        <div
          className={cn(
            'pointer-events-none absolute top-1/2 right-1 flex -translate-y-1/2 items-center gap-1 group-hover:pointer-events-auto focus-within:pointer-events-auto',
            isRunning && 'right-7',
          )}
        >
          <button
            aria-label={t('conversations.delete', { title })}
            className="grid size-6 place-items-center rounded-md bg-muted/80 text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
            onClick={() => onDeleteConversation(conversation.id)}
            title={t('conversations.delete', { title })}
            type="button"
          >
            <Trash2 className="size-3.5" strokeWidth={1.75} />
          </button>
        </div>
      </div>
    </li>
  )
}

function conversationsHaveRunning(
  conversations: readonly ProjectConversation[],
  runningConversationIds: Set<string>,
) {
  return conversations.some((conversation) => runningConversationIds.has(conversation.id))
}

function mergeCreatedConversationsWithFetched(
  createdConversations: readonly ProjectConversation[],
  fetchedConversations: readonly ProjectConversation[],
) {
  const fetchedConversationIds = new Set(
    fetchedConversations.map((conversation) => conversation.id),
  )

  return [
    ...createdConversations.filter((conversation) => !fetchedConversationIds.has(conversation.id)),
    ...fetchedConversations,
  ]
}

function formatSidebarRelativeTime(value: string, t: ReturnType<typeof useTranslation>['t']) {
  const updatedAt = new Date(value).getTime()
  if (!Number.isFinite(updatedAt)) {
    return ''
  }

  const elapsedMs = Math.max(0, Date.now() - updatedAt)
  const elapsedMinutes = Math.max(1, Math.floor(elapsedMs / 60_000))

  if (elapsedMinutes < 60) {
    return t('relativeTime.minutes', { count: elapsedMinutes })
  }

  const elapsedHours = Math.floor(elapsedMinutes / 60)
  if (elapsedHours < 24) {
    return t('relativeTime.hours', { count: elapsedHours })
  }

  const elapsedDays = Math.floor(elapsedHours / 24)
  if (elapsedDays < 7) {
    return t('relativeTime.days', { count: elapsedDays })
  }

  const elapsedWeeks = Math.floor(elapsedDays / 7)
  if (elapsedWeeks < 5) {
    return t('relativeTime.weeks', { count: elapsedWeeks })
  }

  return t('relativeTime.months', { count: Math.floor(elapsedDays / 30) })
}

function removeConversationFromList(
  current: ListConversationsResponse | undefined,
  conversationId: string,
) {
  if (!current) {
    return current
  }

  return {
    conversations: current.conversations.filter(
      (conversation) => conversation.id !== conversationId,
    ),
  }
}

function removeConversationFromProjectGroups(
  current: ListProjectConversationGroupsResponse | undefined,
  conversationId: string,
) {
  if (!current) {
    return current
  }

  return {
    ...current,
    groups: current.groups.map((group) => ({
      ...group,
      conversations: group.conversations.filter(
        (conversation) => conversation.id !== conversationId,
      ),
    })),
  }
}

function addConversationToProjectGroup(
  current: ListProjectConversationGroupsResponse | undefined,
  projectPath: string,
  conversation: ProjectConversation,
) {
  if (!current) {
    return current
  }

  return {
    ...current,
    groups: current.groups.map((group) =>
      group.project.path === projectPath
        ? {
            ...group,
            conversations: [
              conversation,
              ...group.conversations.filter((current) => current.id !== conversation.id),
            ],
          }
        : group,
    ),
  }
}

function getPinnedProjectConversations(
  groups: ProjectConversationGroup[],
  pinnedConversationIds: Set<string>,
): PinnedProjectConversation[] {
  if (pinnedConversationIds.size === 0) {
    return []
  }

  return groups.flatMap((group) =>
    group.conversations
      .filter((conversation) => pinnedConversationIds.has(conversation.id))
      .map((conversation) => ({
        conversation,
        projectPath: group.project.path,
      })),
  )
}

function removePinnedConversationsFromGroups(
  groups: ProjectConversationGroup[],
  pinnedConversationIds: Set<string>,
) {
  if (pinnedConversationIds.size === 0) {
    return groups
  }

  return groups.map((group) => ({
    ...group,
    conversations: group.conversations.filter(
      (conversation) => !pinnedConversationIds.has(conversation.id),
    ),
  }))
}

function removePinnedConversationId(current: Set<string>, conversationId: string) {
  if (!current.has(conversationId)) {
    return current
  }

  const next = new Set(current)
  next.delete(conversationId)
  return next
}

function readPinnedConversationIds() {
  try {
    const rawValue = window.localStorage.getItem(PINNED_CONVERSATION_IDS_STORAGE_KEY)
    if (!rawValue) {
      return new Set<string>()
    }
    const parsedValue = JSON.parse(rawValue)
    if (!Array.isArray(parsedValue)) {
      return new Set<string>()
    }
    return new Set(parsedValue.filter((value): value is string => typeof value === 'string'))
  } catch {
    return new Set<string>()
  }
}

function writePinnedConversationIds(pinnedConversationIds: Set<string>) {
  try {
    window.localStorage.setItem(
      PINNED_CONVERSATION_IDS_STORAGE_KEY,
      JSON.stringify([...pinnedConversationIds]),
    )
  } catch {
    // localStorage can be unavailable in constrained webviews; pinning remains session-local.
  }
}

function removeDeletedProjectFromCache(
  queryClient: ReturnType<typeof useQueryClient>,
  response: DeleteProjectResponse,
) {
  queryClient.setQueryData<ListProjectConversationGroupsResponse>(
    PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
    (current) => {
      if (!current) {
        return current
      }

      return {
        ...current,
        activePath: response.activePath,
        groups: current.groups.filter((group) => group.project.path !== response.path),
      }
    },
  )
}
