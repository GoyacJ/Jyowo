import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useRouterState } from '@tanstack/react-router'
import { ChevronsLeft, ChevronsRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { conversationQueryKeys } from '@/features/conversation/use-conversation'
import { useUiStore } from '@/shared/state/ui-store'
import {
  createConversation as createConversationCommand,
  deleteConversation as deleteConversationCommand,
  type ListConversationsResponse,
  listConversations,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { CommandPalette, type CommandPaletteAction } from './CommandPalette'
import { ConversationList } from './ConversationList'
import { ProjectSelector } from './ProjectSelector'
import { useActiveProjectPath } from './use-active-project-path'

type SidebarNavProps = {
  compact?: boolean
}

export function SidebarNav({ compact = false }: SidebarNavProps) {
  const { t } = useTranslation(['shell', 'conversation'])
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const selectedConversationId = useRouterState({
    select: (state) => state.location.search.conversationId,
  })
  const clearActiveRun = useUiStore((state) => state.clearActiveRun)
  const activeProjectPathQuery = useActiveProjectPath()
  const workspacePath = activeProjectPathQuery.data ?? null
  const workspaceKey = workspacePath ?? 'none'
  const conversationsQuery = useQuery({
    enabled: !activeProjectPathQuery.isLoading,
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
      void navigate({ search: { conversationId: response.conversation.id }, to: '/' }).then(() => {
        window.setTimeout(() => {
          document.querySelector<HTMLTextAreaElement>('textarea')?.focus()
        }, 0)
      })
      void queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })
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
      await queryClient.invalidateQueries({ queryKey: conversationQueryKeys.list(workspaceKey) })

      if (selectedConversationId === conversationId) {
        void navigate({ to: '/' })
      }
    },
  })
  const sidebarCollapsed = useUiStore((state) => state.sidebarCollapsed)
  const setSidebarCollapsed = useUiStore((state) => state.setSidebarCollapsed)
  const setInspectorOpen = useUiStore((state) => state.setInspectorOpen)
  const conversationListErrorMessage = createConversationMutation.error
    ? getCommandErrorMessage(createConversationMutation.error)
    : deleteConversationMutation.error
      ? getCommandErrorMessage(deleteConversationMutation.error)
      : conversationsQuery.error
        ? getCommandErrorMessage(conversationsQuery.error)
        : undefined

  function selectConversation(conversationId: string) {
    void navigate({ search: { conversationId }, to: '/' })
  }

  function focusComposerForNewConversation() {
    createConversationMutation.mutate()
  }

  function deleteConversation(conversationId: string) {
    deleteConversationMutation.mutate(conversationId)
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
        <ProjectSelector compact />
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
      <CommandPalette onAction={runCommand} />
      <div className="flex shrink-0 items-center gap-2 px-3 pt-3">
        <ProjectSelector />
        <button
          aria-label={t('actions.collapseSidebar')}
          className="grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setSidebarCollapsed(true)}
          type="button"
        >
          <ChevronsLeft className="size-4" />
        </button>
      </div>
      <ConversationList
        activeConversationId={selectedConversationId}
        conversations={conversationsQuery.data?.conversations ?? []}
        disabled={false}
        errorMessage={conversationListErrorMessage}
        isLoading={activeProjectPathQuery.isLoading || conversationsQuery.isLoading}
        onDeleteConversation={deleteConversation}
        onNewConversation={focusComposerForNewConversation}
        onSelectConversation={selectConversation}
      />
    </nav>
  )
}
