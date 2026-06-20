import { useQuery } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import {
  Bot,
  ChevronsRight,
  CircleDot,
  FileText,
  Folder,
  Home,
  MessageSquare,
  Pencil,
  Settings,
  Wrench,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'
import { listConversations } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { ConversationList } from './ConversationList'
import { WorkspaceSearch } from './WorkspaceSearch'

const primaryNavigationItems = [
  { id: 'Home', labelKey: 'nav.home', icon: Home, to: '/' },
  { id: 'Conversations', labelKey: 'nav.conversations', icon: MessageSquare, to: '/' },
  { id: 'Projects', labelKey: 'nav.projects', icon: Folder, to: '/' },
  { id: 'Artifacts', labelKey: 'nav.artifacts', icon: FileText, to: '/artifacts' },
  { id: 'Agents', labelKey: 'nav.agents', icon: Bot, to: '/' },
  { id: 'Tools', labelKey: 'nav.tools', icon: Wrench, to: '/settings' },
]

type SidebarDestination = (typeof primaryNavigationItems)[number]['id'] | 'Settings'

export function SidebarNav() {
  const { t } = useTranslation(['shell', 'conversation'])
  const [searchTerm, setSearchTerm] = useState('')
  const [activeDestination, setActiveDestination] = useState<SidebarDestination>('Conversations')
  const searchInputRef = useRef<HTMLInputElement>(null)
  const commandClient = useCommandClient()
  const navigate = useNavigate()
  const currentPath = useRouterState({
    select: (state) => state.location.pathname,
  })
  const selectedConversationId = useRouterState({
    select: (state) => state.location.search.conversationId,
  })
  const conversationsQuery = useQuery({
    queryKey: ['conversation', 'list'],
    queryFn: () => listConversations(commandClient),
  })
  const workspaceContextQuery = useQuery({
    queryKey: ['workspace', 'context-summary'],
    queryFn: () => commandClient.getContextSnapshot({}),
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const setActivityRailCollapsed = useUiStore((state) => state.setActivityRailCollapsed)
  const setActivityRailExpanded = useUiStore((state) => state.setActivityRailExpanded)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const filteredConversations = useMemo(() => {
    const normalizedSearch = searchTerm.trim().toLowerCase()
    const conversations = conversationsQuery.data?.conversations ?? []

    if (!normalizedSearch) {
      return conversations
    }

    return conversations.filter((conversation) => {
      const preview = conversation.lastMessagePreview ?? ''

      return (
        conversation.title.toLowerCase().includes(normalizedSearch) ||
        preview.toLowerCase().includes(normalizedSearch)
      )
    })
  }, [conversationsQuery.data?.conversations, searchTerm])
  const activeConversationId =
    selectedConversationId ?? conversationsQuery.data?.conversations[0]?.id
  const workspaceProject = workspaceContextQuery.data?.project?.trim() || t('workspace')
  const workspacePath = workspaceContextQuery.data?.path?.trim() || t('localWorkspace')
  const workspaceInitials = getWorkspaceInitials(workspaceProject)

  useEffect(() => {
    if (currentPath === '/artifacts') {
      setActiveDestination('Artifacts')
      return
    }

    if (currentPath === '/settings') {
      setActiveDestination('Settings')
      return
    }

    if (currentPath === '/') {
      setActiveDestination('Conversations')
    }
  }, [currentPath])

  function navigateTo(to: string) {
    void navigate({ to })
  }

  function selectConversation(conversationId: string) {
    void navigate({ search: { conversationId }, to: '/' })
  }

  function focusComposerForNewConversation() {
    clearActiveRun()
    void navigate({ to: '/' }).then(() => {
      window.setTimeout(() => {
        document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
      }, 0)
    })
  }

  function runCommand(action: CommandPaletteAction) {
    if (action === 'new-conversation') {
      focusComposerForNewConversation()
      return
    }

    if (action === 'search-files') {
      searchInputRef.current?.focus()
      return
    }

    if (action === 'view-activity') {
      setActivityRailCollapsed(false)
      setActivityRailExpanded(true)
      return
    }

    if (action === 'open-artifact') {
      setActiveDestination('Artifacts')
      navigateTo('/artifacts')
      return
    }

    if (action === 'open-evals') {
      navigateTo('/evals')
      return
    }

    if (action === 'settings') {
      setActiveDestination('Settings')
      setInspectorOpen(true)
      navigateTo('/settings')
    }
  }

  if (sidebarCollapsed) {
    return (
      <nav
        aria-label={t('workspace')}
        className="flex min-h-0 flex-col items-center border-border border-r bg-muted/45 py-4"
        data-collapsed="true"
      >
        <CommandPalette onAction={runCommand} />
        <button
          aria-label={t('actions.expandSidebar')}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(false)}
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
      <CommandPalette onAction={runCommand} />
      <div className="flex h-14 items-center justify-between gap-2 px-4">
        <span className="flex min-w-0 items-center gap-2.5">
          <CircleDot className="size-5 shrink-0 text-foreground" />
          <span className="truncate font-semibold text-sm">{workspaceProject}</span>
        </span>
        <button
          aria-label={t('actions.newConversation')}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={focusComposerForNewConversation}
          type="button"
        >
          <Pencil data-icon="button" className="size-4" />
        </button>
      </div>
      <div className="px-3">
        <WorkspaceSearch
          inputRef={searchInputRef}
          onChange={(event) => setSearchTerm(event.target.value)}
          value={searchTerm}
        />
      </div>
      <ConversationList
        activeConversationId={activeConversationId}
        conversations={filteredConversations}
        errorMessage={
          conversationsQuery.error ? getCommandErrorMessage(conversationsQuery.error) : undefined
        }
        isLoading={conversationsQuery.isLoading}
        onSelectConversation={selectConversation}
      />
      <div className="mt-6 flex-1 px-3">
        <ul className="flex flex-col gap-1">
          {primaryNavigationItems.map(({ icon: Icon, id, labelKey, to }) => (
            <li key={id}>
              <button
                aria-current={activeDestination === id ? 'page' : undefined}
                className="flex w-full items-center gap-3 rounded-md px-3 py-1.5 text-left text-sm text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-surface data-[active=true]:text-foreground"
                data-active={activeDestination === id}
                onClick={() => {
                  setActiveDestination(id)
                  navigateTo(to)
                }}
                type="button"
              >
                <Icon className="size-4" />
                {t(labelKey)}
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="border-border border-t p-3">
        <button
          aria-current={activeDestination === 'Settings' ? 'page' : undefined}
          className="mb-3 flex w-full items-center gap-3 rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-surface data-[active=true]:text-foreground"
          data-active={activeDestination === 'Settings'}
          onClick={() => {
            setActiveDestination('Settings')
            setInspectorOpen(true)
            navigateTo('/settings')
          }}
          type="button"
        >
          <Settings className="size-4" />
          {t('nav.settings')}
        </button>
        <div className="flex w-full items-center justify-between rounded-md px-3 py-1.5 text-left">
          <span className="flex min-w-0 items-center gap-3">
            <span className="grid size-8 shrink-0 place-items-center rounded-full bg-accent/20 text-sm">
              {workspaceInitials}
            </span>
            <span className="min-w-0">
              <span className="block truncate font-medium text-sm">{workspaceProject}</span>
              <span className="block truncate text-muted-foreground text-xs">{workspacePath}</span>
            </span>
          </span>
        </div>
      </div>
    </nav>
  )
}

function getWorkspaceInitials(project: string) {
  const initials = project
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join('')

  return initials || 'J'
}
