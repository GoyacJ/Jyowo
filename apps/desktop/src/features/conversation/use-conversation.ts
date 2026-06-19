import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { useUiStore } from '@/shared/state/ui-store'
import {
  type GetConversationResponse,
  getConversation,
  listConversations,
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

export function useConversation(options: UseConversationOptions = {}) {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const setActiveRun = useUiStore((state) => state.setActiveRun)
  const includeDetail = options.includeDetail ?? true

  const conversationsQuery = useQuery({
    queryKey: conversationQueryKeys.list(),
    queryFn: () => listConversations(commandClient),
  })

  const selectedConversationId =
    options.conversationId ?? conversationsQuery.data?.conversations[0]?.id

  const conversationQuery = useQuery({
    queryKey: conversationQueryKeys.detail(selectedConversationId ?? 'none'),
    queryFn: () => getConversation(selectedConversationId ?? '', commandClient),
    enabled: includeDetail && Boolean(selectedConversationId),
  })

  const startRunMutation = useMutation({
    mutationFn: (prompt: string) => {
      if (!selectedConversationId) {
        throw new Error('No conversation selected')
      }

      return startRun(
        {
          conversationId: selectedConversationId,
          prompt,
        },
        commandClient,
      )
    },
    onSuccess: async (response) => {
      if (selectedConversationId) {
        setActiveRun({
          conversationId: selectedConversationId,
          runId: response.runId,
        })
        await queryClient.invalidateQueries({
          queryKey: conversationQueryKeys.detail(selectedConversationId),
        })
      }
    },
  })

  const isEmpty = conversationsQuery.isSuccess && conversationsQuery.data.conversations.length === 0

  return {
    conversation: conversationQuery.data?.conversation ?? null,
    conversations: conversationsQuery.data?.conversations ?? [],
    error: conversationsQuery.error ?? conversationQuery.error,
    isEmpty,
    isLoading:
      conversationsQuery.isLoading ||
      (includeDetail && Boolean(selectedConversationId) && conversationQuery.isLoading),
    isSubmitting: startRunMutation.isPending,
    selectedConversationId,
    submitError: startRunMutation.error,
    submitPrompt: startRunMutation.mutateAsync,
  }
}
