import type { QueryClient } from '@tanstack/react-query'

import { uiStore } from '@/shared/state/ui-store'

type WorkspaceNavigate = (options: {
  replace?: boolean
  search: Record<string, never>
  to: string
}) => void | Promise<void>

export async function onProjectWorkspaceChanged(
  queryClient: QueryClient,
  navigate: WorkspaceNavigate,
) {
  uiStore.getState().clearActiveRun()
  uiStore.getState().clearTimelineScrollRequest()
  queryClient.removeQueries({ queryKey: ['conversation'] })
  await navigate({ replace: true, search: {}, to: '/' })
  await queryClient.invalidateQueries()
}
