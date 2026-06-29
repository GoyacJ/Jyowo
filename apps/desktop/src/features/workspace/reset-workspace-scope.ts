import type { QueryClient } from '@tanstack/react-query'

import { uiStore } from '@/shared/state/ui-store'
import {
  advanceWorkspaceScopeGeneration,
  providerSettingsQueryKey,
  removeProviderSettingsSaveMutations,
} from '@/shared/state/workspace-scope'

type WorkspaceNavigate = (options: {
  replace?: boolean
  search: Record<string, never>
  to: string
}) => void | Promise<void>

export async function onProjectWorkspaceChanged(
  queryClient: QueryClient,
  navigate: WorkspaceNavigate,
) {
  advanceWorkspaceScopeGeneration(queryClient)
  removeProviderSettingsSaveMutations(queryClient)
  uiStore.getState().clearActiveRun()
  uiStore.getState().clearTimelineScrollRequest()
  queryClient.removeQueries({ queryKey: ['conversation'] })
  queryClient.removeQueries({ queryKey: providerSettingsQueryKey })
  await navigate({ replace: true, search: {}, to: '/' })
  await queryClient.invalidateQueries()
}
