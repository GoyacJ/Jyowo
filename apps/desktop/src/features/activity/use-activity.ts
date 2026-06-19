import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'

import type { RunEvent } from '@/shared/events/run-event-schema'
import { type ListActivityRequest, listActivity, resolvePermission } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

import type { CurrentRunStatus } from './ActivityItem'
import type { RunEventDetailsModel } from './RunEventDetails'
import { toRunEventViewModels } from './run-event-view-model'
import type { UsageSummaryModel } from './UsageSummary'

const activityQueryKeys = {
  all: ['activity'] as const,
  list: (request: Partial<ListActivityRequest>) =>
    [
      ...activityQueryKeys.all,
      {
        conversationId: request.conversationId ?? null,
        runId: request.runId ?? null,
      },
    ] as const,
}

export function useActivity(request: Partial<ListActivityRequest> = {}) {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const resolvingPermissionIdRef = useRef<string | undefined>(undefined)
  const [resolvingPermissionId, setResolvingPermissionId] = useState<string | undefined>()
  const activityRequest = request.conversationId
    ? {
        conversationId: request.conversationId,
        runId: request.runId,
      }
    : null
  const activityQuery = useQuery({
    enabled: Boolean(activityRequest),
    queryKey: activityQueryKeys.list(request),
    queryFn: () => {
      if (!activityRequest) {
        throw new Error('conversationId is required for activity listing')
      }

      return listActivity(activityRequest, commandClient)
    },
  })
  const resolvePermissionMutation = useMutation({
    mutationFn: (permission: { decision: 'approve' | 'deny'; requestId: string }) =>
      resolvePermission(permission, commandClient),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: activityQueryKeys.all }),
    onSettled: () => {
      resolvingPermissionIdRef.current = undefined
      setResolvingPermissionId(undefined)
    },
  })

  const viewModels = activityQuery.data ? toRunEventViewModels(activityQuery.data.events) : []
  const items = viewModels.map((viewModel) => viewModel.activityItem)
  const activeDetails: RunEventDetailsModel | undefined =
    viewModels.find((viewModel) =>
      viewModel.details?.permissions?.some((permission) => permission.state === 'pending'),
    )?.details ?? viewModels.find((viewModel) => viewModel.details)?.details
  const latestItem = items.at(-1)
  const usageSummary = latestUsageSummary(activityQuery.data?.events ?? [])
  const currentRun: CurrentRunStatus | undefined = latestItem
    ? {
        label: 'Current run',
        status: latestItem.status,
      }
    : undefined

  function submitPermissionDecision(decision: 'approve' | 'deny', requestId: string) {
    if (resolvingPermissionIdRef.current) {
      return
    }

    resolvingPermissionIdRef.current = requestId
    setResolvingPermissionId(requestId)
    resolvePermissionMutation.mutate({ decision, requestId })
  }

  return {
    activeDetails,
    approvePermission: (requestId: string) => submitPermissionDecision('approve', requestId),
    currentRun,
    denyPermission: (requestId: string) => submitPermissionDecision('deny', requestId),
    error: activityQuery.error,
    events: activityQuery.data?.events ?? [],
    isLoading: activityQuery.isLoading,
    items,
    resolvingPermissionId,
    usageSummary,
  }
}

function latestUsageSummary(events: RunEvent[]): UsageSummaryModel | undefined {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index]

    if (event?.type === 'run.ended' && event.payload?.usage) {
      return event.payload.usage
    }
  }

  return undefined
}
