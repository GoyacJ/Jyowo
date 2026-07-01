import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import type {
  BackgroundAgentIdRequest,
  SendBackgroundAgentInputRequest,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

const backgroundAgentQueryKeys = {
  all: ['background-agents'] as const,
  list: () => [...backgroundAgentQueryKeys.all, 'list'] as const,
}

export function useBackgroundAgents() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const listQuery = useQuery({
    queryFn: () => commandClient.listBackgroundAgents({ includeArchived: true }),
    queryKey: backgroundAgentQueryKeys.list(),
  })

  async function invalidateAgents() {
    await queryClient.invalidateQueries({ queryKey: backgroundAgentQueryKeys.all })
  }

  const pauseMutation = useMutation({
    mutationFn: (request: BackgroundAgentIdRequest) => commandClient.pauseBackgroundAgent(request),
    onSuccess: invalidateAgents,
  })
  const resumeMutation = useMutation({
    mutationFn: (request: BackgroundAgentIdRequest) => commandClient.resumeBackgroundAgent(request),
    onSuccess: invalidateAgents,
  })
  const cancelMutation = useMutation({
    mutationFn: (request: BackgroundAgentIdRequest) => commandClient.cancelBackgroundAgent(request),
    onSuccess: invalidateAgents,
  })
  const sendInputMutation = useMutation({
    mutationFn: (request: SendBackgroundAgentInputRequest) =>
      commandClient.sendBackgroundAgentInput(request),
    onSuccess: invalidateAgents,
  })
  const archiveMutation = useMutation({
    mutationFn: (request: BackgroundAgentIdRequest) =>
      commandClient.archiveBackgroundAgent(request),
    onSuccess: invalidateAgents,
  })
  const deleteMutation = useMutation({
    mutationFn: (request: BackgroundAgentIdRequest) => commandClient.deleteBackgroundAgent(request),
    onSuccess: invalidateAgents,
  })

  return {
    archiveMutation,
    cancelMutation,
    deleteMutation,
    listQuery,
    pauseMutation,
    resumeMutation,
    sendInputMutation,
  }
}
