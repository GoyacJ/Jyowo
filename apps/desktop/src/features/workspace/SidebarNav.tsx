import { useNavigate, useRouterState } from '@tanstack/react-router'
import {
  Bot,
  ChevronDown,
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

import { readUiPreferences, writeUiPreferences } from '@/shared/local-store/ui-preferences-store'
import { useUiStore } from '@/shared/state/ui-store'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { ConversationList } from './ConversationList'
import { prototypeRecentConversations } from './prototype-data'
import { WorkspaceSearch } from './WorkspaceSearch'
import { type WorkspaceOption, WorkspaceSelector } from './WorkspaceSelector'

const primaryNavigationItems = [
  { label: 'Home', icon: Home, to: '/' },
  { label: 'Conversations', icon: MessageSquare, to: '/' },
  { label: 'Projects', icon: Folder, to: '/' },
  { label: 'Artifacts', icon: FileText, to: '/artifacts' },
  { label: 'Agents', icon: Bot, to: '/' },
  { label: 'Tools', icon: Wrench, to: '/settings' },
]

type SidebarDestination = (typeof primaryNavigationItems)[number]['label'] | 'Settings'

const prototypeWorkspaces: WorkspaceOption[] = [
  {
    name: 'Jyowo',
    path: 'Current project',
    ref: 'local:current',
  },
  {
    name: 'Design sandbox',
    path: 'Local prototype workspace',
    ref: 'local:design-sandbox',
  },
]

export function SidebarNav() {
  const [searchTerm, setSearchTerm] = useState('')
  const [activeDestination, setActiveDestination] = useState<SidebarDestination>('Conversations')
  const searchInputRef = useRef<HTMLInputElement>(null)
  const navigate = useNavigate()
  const currentPath = useRouterState({
    select: (state) => state.location.pathname,
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const selectedWorkspaceRef = useUiStore((state) => state.selectedWorkspaceRef)
  const setSelectedWorkspaceRef = useUiStore((state) => state.setSelectedWorkspaceRef)
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const setActivityRailCollapsed = useUiStore((state) => state.setActivityRailCollapsed)
  const setActivityRailExpanded = useUiStore((state) => state.setActivityRailExpanded)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const filteredConversations = useMemo(() => {
    const normalizedSearch = searchTerm.trim().toLowerCase()

    if (!normalizedSearch) {
      return prototypeRecentConversations
    }

    return prototypeRecentConversations.filter((conversation) =>
      conversation.toLowerCase().includes(normalizedSearch),
    )
  }, [searchTerm])

  useEffect(() => {
    let mounted = true

    void readUiPreferences()
      .then((preferences) => {
        if (mounted && preferences.lastSelectedWorkspaceRef) {
          setSelectedWorkspaceRef(preferences.lastSelectedWorkspaceRef)
        }
      })
      .catch(() => {})

    return () => {
      mounted = false
    }
  }, [setSelectedWorkspaceRef])

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

  function selectWorkspace(workspace: WorkspaceOption) {
    setSelectedWorkspaceRef(workspace.ref)
    void writeUiPreferences({ lastSelectedWorkspaceRef: workspace.ref }).catch(() => {})
  }

  function focusComposerForNewConversation() {
    clearActiveRun()
    void navigate({ to: '/' }).then(() => {
      window.setTimeout(() => {
        document
          .querySelector<HTMLTextAreaElement>(
            'textarea[placeholder="Ask Jyowo anything about this project..."]',
          )
          ?.focus()
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
        aria-label="Workspace"
        className="flex min-h-0 flex-col items-center border-border border-r bg-muted/45 py-4"
        data-collapsed="true"
      >
        <button
          aria-label="Expand sidebar"
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
      aria-label="Workspace"
      className="flex min-h-0 flex-col border-border border-r bg-muted/45"
      data-collapsed="false"
    >
      <CommandPalette onAction={runCommand} />
      <div className="flex h-16 items-center justify-between px-5">
        <span className="flex items-center gap-3 font-semibold text-sm">
          <CircleDot className="size-5 text-foreground" />
          Jyowo
        </span>
        <button
          aria-label="New conversation"
          className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={focusComposerForNewConversation}
          type="button"
        >
          <Pencil data-icon="button" className="size-4" />
        </button>
      </div>
      <div className="px-4">
        <WorkspaceSearch
          inputRef={searchInputRef}
          onChange={(event) => setSearchTerm(event.target.value)}
          value={searchTerm}
        />
      </div>
      <WorkspaceSelector
        onSelect={selectWorkspace}
        selectedWorkspaceRef={selectedWorkspaceRef}
        workspaces={prototypeWorkspaces}
      />
      <ConversationList
        activeConversation="Build the desktop foundation"
        conversations={filteredConversations}
      />
      <div className="mt-8 flex-1 px-3">
        <ul className="flex flex-col gap-1">
          {primaryNavigationItems.map(({ icon: Icon, label, to }) => (
            <li key={label}>
              <button
                aria-current={activeDestination === label ? 'page' : undefined}
                className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-surface data-[active=true]:text-foreground"
                data-active={activeDestination === label}
                onClick={() => {
                  setActiveDestination(label)
                  navigateTo(to)
                }}
                type="button"
              >
                <Icon className="size-4" />
                {label}
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="border-border border-t p-3">
        <button
          aria-current={activeDestination === 'Settings' ? 'page' : undefined}
          className="mb-3 flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-surface data-[active=true]:text-foreground"
          data-active={activeDestination === 'Settings'}
          onClick={() => {
            setActiveDestination('Settings')
            setInspectorOpen(true)
            navigateTo('/settings')
          }}
          type="button"
        >
          <Settings className="size-4" />
          Settings
        </button>
        <button
          className="flex w-full items-center justify-between rounded-md px-3 py-2 text-left hover:bg-muted"
          type="button"
        >
          <span className="flex min-w-0 items-center gap-3">
            <span className="grid size-8 shrink-0 place-items-center rounded-full bg-accent/20 text-sm">
              JD
            </span>
            <span className="min-w-0">
              <span className="block truncate font-medium text-sm">Jane Doe</span>
              <span className="block truncate text-muted-foreground text-xs">Local workspace</span>
            </span>
          </span>
          <ChevronDown className="size-4 -rotate-90 text-muted-foreground" />
        </button>
      </div>
    </nav>
  )
}
