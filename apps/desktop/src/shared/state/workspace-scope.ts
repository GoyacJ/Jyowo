import type { QueryClient } from '@tanstack/react-query'

export const providerSettingsQueryKey = ['provider-settings'] as const
const providerSettingsSaveMutationKey = ['provider-settings', 'save'] as const

const workspaceScopeGenerationQueryKey = ['workspace-scope-generation'] as const

function getWorkspaceScopeGeneration(queryClient: QueryClient) {
  return queryClient.getQueryData<number>(workspaceScopeGenerationQueryKey) ?? 0
}

export function advanceWorkspaceScopeGeneration(queryClient: QueryClient) {
  const nextGeneration = getWorkspaceScopeGeneration(queryClient) + 1
  queryClient.setQueryData(workspaceScopeGenerationQueryKey, nextGeneration)
  return nextGeneration
}

export function removeProviderSettingsSaveMutations(queryClient: QueryClient) {
  const mutationCache = queryClient.getMutationCache()
  for (const mutation of mutationCache.findAll({ mutationKey: providerSettingsSaveMutationKey })) {
    mutationCache.remove(mutation)
  }
}
