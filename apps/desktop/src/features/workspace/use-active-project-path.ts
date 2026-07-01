import { useQuery } from '@tanstack/react-query'

import { listProjects } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export function useActiveProjectPath({ enabled = true }: { enabled?: boolean } = {}) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
    queryFn: () => listProjects(commandClient),
    queryKey: ['projects', 'list'],
    select: (data) => data.activePath,
  })
}
