import { useQuery } from '@tanstack/react-query'

import type { TypedUlid } from '@/generated/daemon-protocol'
import { DEFAULT_MEMORY_TENANT_ID } from '@/features/memory/memory-types'
import {
  type GetContextSnapshotRequest,
  type GetContextSnapshotResponse,
  getContextSnapshot,
} from '@/shared/tauri/commands'
import { useCommandClient, useDaemonClient } from '@/shared/tauri/react'

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
  options: { enabled?: boolean; workspaceRoot?: string } = {},
) {
  const commandClient = useCommandClient()
  const daemonClient = useDaemonClient()
  const contextQuery = useQuery({
    enabled: options.enabled ?? true,
    queryKey: contextSnapshotQueryKeys.detail(request),
    queryFn: () => getContextSnapshot(request, commandClient),
  })
  const previewQuery = useQuery({
    enabled: Boolean((options.enabled ?? true) && request.conversationId && request.runId),
    queryKey: [
      ...contextSnapshotQueryKeys.detail(request),
      'model-request-preview',
      options.workspaceRoot ?? null,
    ],
    queryFn: () =>
      daemonClient.getModelRequestPreview(options.workspaceRoot, {
        run_id: request.runId as TypedUlid,
        session_id: request.conversationId as TypedUlid,
        tenant_id: DEFAULT_MEMORY_TENANT_ID,
      }),
  })

  return {
    context: contextQuery.data ? toWorkspaceContext(contextQuery.data) : null,
    error: contextQuery.error,
    isLoading: contextQuery.isLoading,
    modelRequestPreview: previewQuery.data?.preview ?? null,
    modelRequestPreviewError: previewQuery.error,
    modelRequestPreviewLoading: previewQuery.isLoading,
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
