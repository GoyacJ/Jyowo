import { useMutation, useQuery } from '@tanstack/react-query'

import { useUiStore } from '@/shared/state/ui-store'
import {
  type GetConversationResponse,
  getConversation,
  listConversations,
  type StartRunRequest,
  startRun,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

const conversationQueryKeys = {
  all: ['conversation'] as const,
  detail: (conversationId: string) =>
    [...conversationQueryKeys.all, 'detail', conversationId] as const,
  list: () => [...conversationQueryKeys.all, 'list'] as const,
}

type UseConversationOptions = {
  includeDetail?: boolean
  conversationId?: string
}

export type ConversationRecord = GetConversationResponse['conversation']
export type ConversationSubmitDraft = Omit<StartRunRequest, 'conversationId'>

export function useConversation(options: UseConversationOptions = {}) {
  const commandClient = useCommandClient()
  const setActiveRun = useUiStore((state) => state.setActiveRun)
  const includeDetail = options.includeDetail ?? true

  const conversationsQuery = useQuery({
    queryKey: conversationQueryKeys.list(),
    queryFn: () => listConversations(commandClient),
  })

  const selectedConversationId =
    options.conversationId ?? conversationsQuery.data?.conversations[0]?.id
  const selectedConversationListed = options.conversationId
    ? conversationsQuery.data?.conversations.some(
        (conversation) => conversation.id === options.conversationId,
      ) === true
    : Boolean(selectedConversationId)
  const isDraft = Boolean(
    options.conversationId && conversationsQuery.isSuccess && !selectedConversationListed,
  )
  const shouldLoadDetail =
    includeDetail &&
    Boolean(selectedConversationId) &&
    (!options.conversationId || selectedConversationListed)

  const conversationQuery = useQuery({
    queryKey: conversationQueryKeys.detail(selectedConversationId ?? 'none'),
    queryFn: () => getConversation(selectedConversationId ?? '', commandClient),
    enabled: shouldLoadDetail,
  })

  const startRunMutation = useMutation({
    mutationFn: (draft: ConversationSubmitDraft) => {
      if (!selectedConversationId) {
        throw new Error('No conversation selected')
      }

      return startRun(
        {
          ...draft,
          conversationId: selectedConversationId,
        },
        commandClient,
      )
    },
    onSuccess: (response) => {
      if (selectedConversationId) {
        setActiveRun({
          conversationId: selectedConversationId,
          runId: response.runId,
        })
      }
    },
  })

  const isEmpty =
    !options.conversationId &&
    conversationsQuery.isSuccess &&
    conversationsQuery.data.conversations.length === 0

  return {
    conversation: conversationQuery.data?.conversation ?? null,
    conversations: conversationsQuery.data?.conversations ?? [],
    error: conversationsQuery.error ?? conversationQuery.error,
    isDraft,
    isEmpty,
    isLoading: conversationsQuery.isLoading || (shouldLoadDetail && conversationQuery.isLoading),
    isSubmitting: startRunMutation.isPending,
    selectedConversationId,
    submitError: startRunMutation.error,
    submitPrompt: startRunMutation.mutateAsync,
  }
}
