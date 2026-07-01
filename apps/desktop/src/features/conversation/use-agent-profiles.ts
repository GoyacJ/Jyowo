import { useQuery } from '@tanstack/react-query'

import { useActiveProjectPath } from '@/features/workspace/use-active-project-path'
import { listAgentProfiles } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

const agentProfileQueryKeys = {
  all: ['agent-profiles'] as const,
  list: (workspacePath: string) => [...agentProfileQueryKeys.all, workspacePath] as const,
}

export function useAgentProfiles({ enabled = true }: { enabled?: boolean } = {}) {
  const commandClient = useCommandClient()
  const activeProjectPathQuery = useActiveProjectPath({ enabled })
  const workspacePath = activeProjectPathQuery.data ?? null
  const workspaceKey = workspacePath ?? 'none'

  const profilesQuery = useQuery({
    enabled: enabled && Boolean(workspacePath),
    queryFn: () => listAgentProfiles(commandClient),
    queryKey: agentProfileQueryKeys.list(workspaceKey),
  })

  return {
    error: activeProjectPathQuery.error ?? profilesQuery.error,
    isEmpty: profilesQuery.isSuccess && profilesQuery.data.profiles.length === 0,
    isLoading: activeProjectPathQuery.isLoading || profilesQuery.isLoading,
    profiles: profilesQuery.data?.profiles ?? [],
    workspacePath,
  }
}
