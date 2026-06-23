import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { ChevronsLeft, ChevronsRight, FileText, Folder, Home, MessageSquare } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useUiStore } from '@/shared/state/ui-store'
import {
  createConversation as createConversationCommand,
  deleteConversation as deleteConversationCommand,
  type ListConversationsResponse,
  listConversations,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import appIconUrl from '../../../src-tauri/icons/32x32.png'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { ConversationList } from './ConversationList'

const primaryNavigationItems = [
  { id: 'Home', labelKey: 'nav.home', icon: Home, to: '/' },
  { id: 'Conversations', labelKey: 'nav.conversations', icon: MessageSquare, to: '/' },
  { id: 'Projects', labelKey: 'nav.projects', icon: Folder, to: '/' },
  { id: 'Artifacts', labelKey: 'nav.artifacts', icon: FileText, to: '/artifacts' },
]

type SidebarDestination = (typeof primaryNavigationItems)[number]['id']

type SidebarNavProps = {
  compact?: boolean
}

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const { t } = useTranslation(['shell', 'conversation'])
  const [activeDestination, setActiveDestination] = useState<SidebarDestination | null>(
    'Conversations',
  )
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const currentPath = useRouterState({
    select: (state) => state.location.pathname,
  })
  const selectedConversationId = useRouterState({
    select: (state) => state.location.search.conversationId,
  })
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const conversationsQuery = useQuery({
    queryKey: ['conversation', 'list'],
    queryFn: () => listConversations(commandClient),
  })
  const createConversationMutation = useMutation({
    mutationFn: () => createConversationCommand(commandClient),
    onSuccess: async (response) => {
      queryClient.setQueryData<ListConversationsResponse>(['conversation', 'list'], (current) => {
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
      })
      void navigate({ search: { conversationId: response.conversation.id }, to: '/' }).then(() => {
        window.setTimeout(() => {
          document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
        }, 0)
      })
      void queryClient.invalidateQueries({ queryKey: ['conversation', 'list'] })
    },
  })
  const deleteConversationMutation = useMutation({
    mutationFn: (conversationId: string) =>
      deleteConversationCommand(conversationId, commandClient),
    onSuccess: async (_, conversationId) => {
      clearActiveRun(conversationId)
      queryClient.setQueryData<ListConversationsResponse>(['conversation', 'list'], (current) => {
        if (!current) {
          return current
        }

        return {
          conversations: current.conversations.filter(
            (conversation) => conversation.id !== conversationId,
          ),
        }
      })
      await queryClient.invalidateQueries({ queryKey: ['conversation', 'list'] })

      if (selectedConversationId === conversationId) {
        void navigate({ to: '/' })
      }
    },
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const activeConversationId =
    selectedConversationId ?? conversationsQuery.data?.conversations[0]?.id
  const conversationListErrorMessage = createConversationMutation.error
    ? getCommandErrorMessage(createConversationMutation.error)
    : conversationsQuery.error
      ? getCommandErrorMessage(conversationsQuery.error)
      : undefined

  useEffect(() => {
    if (currentPath === '/artifacts') {
      setActiveDestination('Artifacts')
      return
    }

    if (currentPath === '/settings' || currentPath === '/skills') {
      setActiveDestination(null)
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
    createConversationMutation.mutate()
  }

  function deleteConversation(conversationId: string) {
    void deleteConversationMutation.mutateAsync(conversationId)
  }

  function runCommand(action: CommandPaletteAction) {
    if (action === 'new-conversation') {
      focusComposerForNewConversation()
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
      setInspectorOpen(true)
      navigateTo('/settings')
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
          aria-label={t('actions.expandSidebar')}
          className="grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(false)}
          title={t('actions.expandSidebar')}
          type="button"
        >
          <ChevronsRight className="size-4" />
        </button>
        <div className="mt-auto flex w-full flex-col items-center gap-1 px-1">
          {primaryNavigationItems.map(({ icon: Icon, id, labelKey, to }) => (
            <button
              aria-current={activeDestination === id ? 'page' : undefined}
              aria-label={t(labelKey)}
              className="grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:bg-surface data-[active=true]:text-foreground"
              data-active={activeDestination === id}
              key={id}
              onClick={() => {
                setActiveDestination(id)
                navigateTo(to)
              }}
              title={t(labelKey)}
              type="button"
            >
              <Icon className="size-4" />
            </button>
          ))}
        </div>
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
      <div className="flex items-center justify-between px-3 pt-3">
        <span
          className="grid size-9 place-items-center rounded-md border border-border bg-surface"
          title={t('localWorkspace')}
        >
          <img alt={t('localWorkspace')} className="size-6" src={appIconUrl} />
        </span>
        <button
          aria-label={t('actions.collapseSidebar')}
          className="grid size-9 place-items-center rounded-md border border-border bg-surface text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(true)}
          type="button"
        >
          <ChevronsLeft className="size-4" />
        </button>
      </div>
      <ConversationList
        activeConversationId={activeConversationId}
        conversations={conversationsQuery.data?.conversations ?? []}
        errorMessage={conversationListErrorMessage}
        isLoading={conversationsQuery.isLoading}
        onDeleteConversation={deleteConversation}
        onNewConversation={focusComposerForNewConversation}
        onSelectConversation={selectConversation}
      />
      <div
        className="mt-auto border-border border-t px-3 py-3"
        data-testid="sidebar-bottom-navigation"
      >
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
    </nav>
  )
}
