import { useQuery } from '@tanstack/react-query'

import {
  type GetContextSnapshotRequest,
  type GetContextSnapshotResponse,
  getContextSnapshot,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

import type { WorkspaceContext } from './ContextPanel'

const contextSnapshotQueryKeys = {
  all: ['context-snapshot'] as const,
  detail: (request: GetContextSnapshotRequest) =>
    [
      ...contextSnapshotQueryKeys.all,
      {
        conversationId: request.conversationId ?? null,
        runId: request.runId ?? null,
      },
    ] as const,
}

export function useContextSnapshot(
  request: GetContextSnapshotRequest = {},
  options: { enabled?: boolean } = {},
) {
  const commandClient = useCommandClient()
  const contextQuery = useQuery({
    enabled: options.enabled ?? true,
    queryKey: contextSnapshotQueryKeys.detail(request),
    queryFn: () => getContextSnapshot(request, commandClient),
  })

  return {
    context: contextQuery.data ? toWorkspaceContext(contextQuery.data) : null,
    error: contextQuery.error,
    isLoading: contextQuery.isLoading,
  }
}

function toWorkspaceContext(snapshot: GetContextSnapshotResponse): WorkspaceContext {
  return {
    activeArtifact: snapshot.activeArtifact ?? undefined,
    decisions: snapshot.decisions,
    files: snapshot.files,
    nextActions: snapshot.nextActions,
    path: snapshot.path,
    project: snapshot.project,
  }
}
