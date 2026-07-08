import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import {
  ChevronDown,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  FolderPlus,
  Plus,
  Search,
  Text,
  Trash2,
} from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { conversationQueryKeys } from '@/features/conversation/use-conversation'
import { cn } from '@/shared/lib/utils'
import { useUiStore } from '@/shared/state/ui-store'
import {
  addProject,
  createConversation as createConversationCommand,
  deleteConversation as deleteConversationCommand,
  type ListProjectConversationGroupsResponse,
  type ListConversationsResponse,
  listConversations,
  listProjectConversationGroups,
  switchProject as switchProjectCommand,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { pickProjectDirectory } from '@/shared/tauri/file-dialog'
import { useCommandClient } from '@/shared/tauri/react'
import { ScrollArea } from '@/shared/ui/scroll-area'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { ConversationList } from './ConversationList'
import { onProjectWorkspaceChanged } from './reset-workspace-scope'
import { useActiveProjectPath } from './use-active-project-path'

type SidebarNavProps = {
  compact?: boolean
}

type ProjectConversationGroup = ListProjectConversationGroupsResponse['groups'][number]
type ProjectConversation = ProjectConversationGroup['conversations'][number]

const PROJECT_CONVERSATION_GROUPS_QUERY_KEY = ['project-conversation-groups'] as const
const DEFAULT_PROJECT_CONVERSATION_LIMIT = 5

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const { t } = useTranslation(['shell', 'conversation'])
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const selectedConversationId = useRouterState({
    select: (state) => state.location.search.conversationId,
  })
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const [searchValue, setSearchValue] = useState('')
  const [expandedProjectPaths, setExpandedProjectPaths] = useState(() => new Set<string>())
  const [navigationError, setNavigationError] = useState<unknown>(null)
  const activeProjectPathQuery = useActiveProjectPath()
  const workspacePath = activeProjectPathQuery.data ?? null
  const workspaceKey = workspacePath ?? 'none'
  const projectConversationGroupsQuery = useQuery({
    queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
    queryFn: () => listProjectConversationGroups(commandClient),
  })
  const projectGroups = projectConversationGroupsQuery.data?.groups ?? []
  const hasProjectGroups = projectGroups.length > 0
  const shouldRenderProjectGroups = projectConversationGroupsQuery.isLoading || hasProjectGroups
  const conversationsQuery = useQuery({
    enabled: projectConversationGroupsQuery.isSuccess && !hasProjectGroups,
    queryKey: conversationQueryKeys.list(workspaceKey),
    queryFn: () => listConversations(commandClient),
  })
  const createConversationMutation = useMutation({
    mutationFn: () => createConversationCommand(commandClient),
    onSuccess: async (response) => {
      queryClient.setQueryData<ListConversationsResponse>(
        conversationQueryKeys.list(workspaceKey),
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
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => addConversationToActiveProjectGroup(current, response.conversation),
      )
      void navigate({ search: { conversationId: response.conversation.id }, to: '/' }).then(() => {
        window.setTimeout(() => {
          document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
        }, 0)
      })
      void queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
      void queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
    },
  })
  const deleteConversationMutation = useMutation({
    mutationFn: (conversationId: string) =>
      deleteConversationCommand(conversationId, commandClient),
    onSuccess: async (_, conversationId) => {
      clearActiveRun(conversationId)
      queryClient.setQueryData<ListConversationsResponse>(
        conversationQueryKeys.list(workspaceKey),
        (current) => {
          if (!current) {
            return current
          }

          return {
            conversations: current.conversations.filter(
              (conversation) => conversation.id !== conversationId,
            ),
          }
        },
      )
      queryClient.setQueryData<ListProjectConversationGroupsResponse>(
        PROJECT_CONVERSATION_GROUPS_QUERY_KEY,
        (current) => removeConversationFromActiveProjectGroup(current, conversationId),
      )
      await queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })

      if (selectedConversationId === conversationId) {
        void navigate({ to: '/' })
      }
    },
  })
  const addProjectMutation = useMutation({
    mutationFn: (path: string) => addProject(path, commandClient),
    onSuccess: async () => {
      await onProjectWorkspaceChanged(queryClient, navigate)
      await queryClient.invalidateQueries({ queryKey: PROJECT_CONVERSATION_GROUPS_QUERY_KEY })
    },
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const conversationListError =
    createConversationMutation.error ??
    deleteConversationMutation.error ??
    addProjectMutation.error ??
    navigationError ??
    projectConversationGroupsQuery.error ??
    conversationsQuery.error
  const conversationListErrorMessage = conversationListError
    ? getCommandErrorMessage(conversationListError)
    : undefined
  const activeProjectPath = projectConversationGroupsQuery.data
    ? projectConversationGroupsQuery.data.activePath
    : workspacePath
  const normalizedSearch = searchValue.trim().toLocaleLowerCase()
  const visibleProjectGroups = useMemo(
    () => filterProjectConversationGroups(projectGroups, normalizedSearch),
    [normalizedSearch, projectGroups],
  )

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
    createConversationMutation.mutate()
  }

  function deleteConversation(conversationId: string) {
    deleteConversationMutation.mutate(conversationId)
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
          <Plus className="size-4" />
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
          <ChevronsRight className="size-4" />
        </button>
      </nav>
    )
  }

  return (
    <nav
      aria-label={t('workspace')}
      className="flex min-h-0 flex-col border-border border-r bg-muted/45"
      data-collapsed="false"
    >
      <div className="flex shrink-0 items-center gap-1.5 px-3 pt-3">
        <button
          className="flex h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md bg-foreground px-2 font-medium text-background text-xs hover:bg-foreground/90"
          onClick={focusComposerForNewConversation}
          type="button"
        >
          <Plus className="size-3.5" />
          <span className="truncate">{t('actions.newConversation')}</span>
        </button>
        <CommandPalette onAction={runCommand} />
        <button
          aria-label={t('projects.new')}
          className="grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => void openProjectDirectory()}
          title={t('projects.new')}
          type="button"
        >
          <FolderPlus className="size-4" />
        </button>
        <button
          aria-label={t('actions.collapseSidebar')}
          className="grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(true)}
          type="button"
        >
          <ChevronsLeft className="size-4" />
        </button>
      </div>
      <div className="px-3 pt-3">
        <label className="relative block">
          <span className="sr-only">{t('conversations.search')}</span>
          <Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-3.5 text-muted-foreground" />
          <input
            aria-label={t('conversations.search')}
            className="h-8 w-full rounded-md border border-border bg-background px-8 text-xs outline-none placeholder:text-muted-foreground focus:border-ring"
            onChange={(event) => setSearchValue(event.target.value)}
            placeholder={t('conversations.searchPlaceholder')}
            type="search"
            value={searchValue}
          />
        </label>
      </div>
      {shouldRenderProjectGroups ? (
        <ProjectConversationGroups
          activeConversationId={selectedConversationId}
          activeProjectPath={activeProjectPath}
          errorMessage={conversationListErrorMessage}
          expandedProjectPaths={expandedProjectPaths}
          groups={visibleProjectGroups}
          isLoading={projectConversationGroupsQuery.isLoading}
          onDeleteConversation={deleteConversation}
          onSelectConversation={(projectPath, conversationId) => {
            void selectProjectConversation(projectPath, conversationId)
          }}
          onToggleProjectExpanded={toggleProjectExpanded}
        />
      ) : (
        <ConversationList
          activeConversationId={selectedConversationId}
          conversations={conversationsQuery.data?.conversations ?? []}
          disabled={false}
          errorMessage={conversationListErrorMessage}
          isLoading={projectConversationGroupsQuery.isLoading || conversationsQuery.isLoading}
          onDeleteConversation={deleteConversation}
          onNewConversation={focusComposerForNewConversation}
          onSelectConversation={selectConversation}
        />
      )}
    </nav>
  )
}

function ProjectConversationGroups({
  activeConversationId,
  activeProjectPath,
  errorMessage,
  expandedProjectPaths,
  groups,
  isLoading,
  onDeleteConversation,
  onSelectConversation,
  onToggleProjectExpanded,
}: {
  activeConversationId?: string
  activeProjectPath: string | null
  errorMessage?: string
  expandedProjectPaths: Set<string>
  groups: ProjectConversationGroup[]
  isLoading: boolean
  onDeleteConversation: (conversationId: string) => void
  onSelectConversation: (projectPath: string, conversationId: string) => void
  onToggleProjectExpanded: (projectPath: string) => void
}) {
  const { t } = useTranslation('shell')

  return (
    <div className="mt-3 flex min-h-0 flex-1 flex-col px-3">
      {isLoading ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.loading')}
        </div>
      ) : null}
      {!isLoading && errorMessage ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-destructive text-xs">{errorMessage}</div>
      ) : null}
      {!isLoading && !errorMessage && groups.length === 0 ? (
        <div className="shrink-0 rounded-md px-2 py-2 text-muted-foreground text-xs">
          {t('conversations.empty')}
        </div>
      ) : null}
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 pr-0.5">
          {groups.map((group) => {
            const isProjectActive = group.project.path === activeProjectPath
            const isExpanded = expandedProjectPaths.has(group.project.path)
            const visibleConversations = isExpanded
              ? group.conversations
              : group.conversations.slice(0, DEFAULT_PROJECT_CONVERSATION_LIMIT)
            const hiddenCount = group.conversations.length - visibleConversations.length

            return (
              <section data-active={isProjectActive} key={group.project.path}>
                <div
                  className="mb-1 flex min-w-0 items-center gap-1 rounded-md px-1.5 py-1 text-muted-foreground data-[active=true]:text-foreground"
                  data-active={isProjectActive}
                >
                  <button
                    aria-label={
                      isExpanded
                        ? t('projects.collapseGroup', { name: group.project.name })
                        : t('projects.expandGroup', { name: group.project.name })
                    }
                    className="grid size-5 shrink-0 place-items-center rounded hover:bg-muted"
                    onClick={() => onToggleProjectExpanded(group.project.path)}
                    type="button"
                  >
                    {isExpanded ? (
                      <ChevronDown className="size-3.5" />
                    ) : (
                      <ChevronRight className="size-3.5" />
                    )}
                  </button>
                  <span className="min-w-0 flex-1 truncate font-medium text-xs">
                    {group.project.name}
                  </span>
                </div>
                {visibleConversations.length ? (
                  <ul className="flex flex-col gap-1">
                    {visibleConversations.map((conversation) => (
                      <ProjectConversationRow
                        activeConversationId={activeConversationId}
                        activeProjectPath={activeProjectPath}
                        conversation={conversation}
                        key={conversation.id}
                        onDeleteConversation={onDeleteConversation}
                        onSelectConversation={onSelectConversation}
                        projectPath={group.project.path}
                      />
                    ))}
                  </ul>
                ) : (
                  <div className="rounded-md px-8 py-1.5 text-muted-foreground text-xs">
                    {t('conversations.empty')}
                  </div>
                )}
                {hiddenCount > 0 ? (
                  <button
                    className="mt-1 rounded-md px-8 py-1 text-left text-muted-foreground text-xs hover:bg-muted hover:text-foreground"
                    onClick={() => onToggleProjectExpanded(group.project.path)}
                    type="button"
                  >
                    {t('projects.showMoreConversations', { count: hiddenCount })}
                  </button>
                ) : null}
              </section>
            )
          })}
        </div>
      </ScrollArea>
    </div>
  )
}

function ProjectConversationRow({
  activeConversationId,
  activeProjectPath,
  conversation,
  onDeleteConversation,
  onSelectConversation,
  projectPath,
}: {
  activeConversationId?: string
  activeProjectPath: string | null
  conversation: ProjectConversation
  onDeleteConversation: (conversationId: string) => void
  onSelectConversation: (projectPath: string, conversationId: string) => void
  projectPath: string
}) {
  const { t } = useTranslation('shell')
  const isProjectActive = projectPath === activeProjectPath
  const isActive = isProjectActive && conversation.id === activeConversationId
  const title = conversation.isEmpty ? t('conversations.defaultTitle') : conversation.title
  const lastMessagePreview = conversation.isEmpty
    ? t('conversations.defaultPreview')
    : conversation.lastMessagePreview

  return (
    <li>
      <div
        className="group grid w-full min-w-0 grid-cols-[minmax(0,1fr)_1.5rem] items-start gap-1 rounded-md pr-1 hover:bg-muted data-[active=true]:bg-accent/10 data-[active=true]:text-foreground"
        data-active={isActive}
      >
        <button
          aria-current={isActive ? 'page' : undefined}
          className="flex min-w-0 items-start overflow-hidden rounded-md px-2 py-1.5 text-left text-xs"
          onClick={() => onSelectConversation(projectPath, conversation.id)}
          type="button"
        >
          <span className="flex w-full min-w-0 gap-2">
            <Text
              aria-hidden="true"
              className={cn(
                'mt-0.5 size-3.5 shrink-0',
                isActive ? 'text-foreground' : 'text-muted-foreground/80',
              )}
              strokeWidth={isActive ? 2 : 1.5}
            />
            <span className="min-w-0 flex-1 overflow-hidden">
              <span className="block truncate">{title}</span>
              {lastMessagePreview ? (
                <span className="mt-0.5 block truncate text-muted-foreground">
                  {lastMessagePreview}
                </span>
              ) : null}
            </span>
          </span>
        </button>
        {isProjectActive ? (
          <button
            aria-label={t('conversations.delete', { title })}
            className="mt-1 grid size-6 place-items-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
            onClick={() => onDeleteConversation(conversation.id)}
            title={t('conversations.delete', { title })}
            type="button"
          >
            <Trash2 className="size-3.5" strokeWidth={1.75} />
          </button>
        ) : (
          <span aria-hidden="true" className="size-6" />
        )}
      </div>
    </li>
  )
}

function filterProjectConversationGroups(
  groups: ProjectConversationGroup[],
  normalizedSearch: string,
) {
  if (!normalizedSearch) {
    return groups
  }

  return groups
    .map((group) => {
      const projectMatches = [group.project.name, group.project.path].some((value) =>
        value.toLocaleLowerCase().includes(normalizedSearch),
      )
      if (projectMatches) {
        return group
      }

      return {
        ...group,
        conversations: group.conversations.filter((conversation) =>
          [conversation.title, conversation.lastMessagePreview ?? ''].some((value) =>
            value.toLocaleLowerCase().includes(normalizedSearch),
          ),
        ),
      }
    })
    .filter((group) => group.conversations.length > 0)
}

function addConversationToActiveProjectGroup(
  current: ListProjectConversationGroupsResponse | undefined,
  conversation: ProjectConversation,
) {
  if (!current?.activePath) {
    return current
  }

  return {
    ...current,
    groups: current.groups.map((group) =>
      group.project.path === current.activePath
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

function removeConversationFromActiveProjectGroup(
  current: ListProjectConversationGroupsResponse | undefined,
  conversationId: string,
) {
  if (!current?.activePath) {
    return current
  }

  return {
    ...current,
    groups: current.groups.map((group) =>
      group.project.path === current.activePath
        ? {
            ...group,
            conversations: group.conversations.filter(
              (conversation) => conversation.id !== conversationId,
            ),
          }
        : group,
    ),
  }
}
