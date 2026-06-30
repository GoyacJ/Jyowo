import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useRef } from 'react'

import {
  getModelUsageSummary,
  listModelProviderCatalog,
  listOfficialQuotaSnapshots,
  listProviderCapabilityRouteOptions,
  listProviderCapabilityRoutes,
  listProviderProbeSnapshots,
  listProviderSettings,
  probeProviderConfig,
  refreshOfficialQuota,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'

import {
  buildModelSettingsPageState,
  type ModelSettingsPageState,
  type ModelSettingsQueryInputs,
  type QuerySlice,
} from './model-settings-view-model'

export class ModelSettingsMutationBlockedError extends Error {
  readonly configId: string
  readonly operation: 'probe' | 'quota'

  constructor(configId: string, operation: 'probe' | 'quota') {
    super(`Model settings ${operation} is already pending for ${configId}`)
    this.name = 'ModelSettingsMutationBlockedError'
    this.configId = configId
    this.operation = operation
  }
}

const modelSettingsQueryKeys = {
  all: ['model-settings'] as const,
  catalog: () => [...modelSettingsQueryKeys.all, 'catalog'] as const,
  providerSettings: () => [...modelSettingsQueryKeys.all, 'provider-settings'] as const,
  probeSnapshots: () => [...modelSettingsQueryKeys.all, 'probe-snapshots'] as const,
  usageSummary: () => [...modelSettingsQueryKeys.all, 'usage-summary'] as const,
  quotaSnapshots: () => [...modelSettingsQueryKeys.all, 'quota-snapshots'] as const,
  capabilityRoutes: () => [...modelSettingsQueryKeys.all, 'capability-routes'] as const,
  capabilityRouteOptions: () =>
    [...modelSettingsQueryKeys.all, 'capability-route-options'] as const,
}

function useModelProviderCatalog() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.catalog(),
    queryFn: () => listModelProviderCatalog(commandClient),
  })
}

function useProviderSettings() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.providerSettings(),
    queryFn: () => listProviderSettings(commandClient),
  })
}

function useProviderProbeSnapshots() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.probeSnapshots(),
    queryFn: () => listProviderProbeSnapshots(commandClient),
  })
}

function useModelUsageSummary() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.usageSummary(),
    queryFn: () => getModelUsageSummary(commandClient),
  })
}

function useOfficialQuotaSnapshots() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.quotaSnapshots(),
    queryFn: () => listOfficialQuotaSnapshots(commandClient),
  })
}

function useProviderCapabilityRoutes() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.capabilityRoutes(),
    queryFn: () => listProviderCapabilityRoutes(commandClient),
  })
}

function useProviderCapabilityRouteOptions() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.capabilityRouteOptions(),
    queryFn: () => listProviderCapabilityRouteOptions(commandClient),
  })
}

function toQuerySlice<T>(query: {
  isLoading: boolean
  isPending: boolean
  isError: boolean
  error: unknown
  data: T | undefined
}): QuerySlice<T> {
  if (query.isPending) {
    return { status: 'loading' }
  }

  if (query.isError) {
    return { status: 'error', safeMessage: getCommandErrorMessage(query.error) }
  }

  if (query.data === undefined) {
    return { status: 'idle' }
  }

  return { status: 'ready', data: query.data }
}

export function useModelSettingsViewModel() {
  const catalogQuery = useModelProviderCatalog()
  const settingsQuery = useProviderSettings()
  const probeSnapshotsQuery = useProviderProbeSnapshots()
  const usageSummaryQuery = useModelUsageSummary()
  const quotaSnapshotsQuery = useOfficialQuotaSnapshots()
  const routesQuery = useProviderCapabilityRoutes()
  const routeOptionsQuery = useProviderCapabilityRouteOptions()
  const probeMutation = useProbeProviderConfig()
  const quotaMutation = useRefreshOfficialQuota()

  const inputs: ModelSettingsQueryInputs = {
    catalog: toQuerySlice(catalogQuery),
    providerSettings: toQuerySlice(settingsQuery),
    probeSnapshots: toQuerySlice(probeSnapshotsQuery),
    usageSummary: toQuerySlice(usageSummaryQuery),
    quotaSnapshots: toQuerySlice(quotaSnapshotsQuery),
    routes: toQuerySlice(routesQuery),
    routeOptions: toQuerySlice(routeOptionsQuery),
  }

  const pageState: ModelSettingsPageState = buildModelSettingsPageState(inputs)

  return {
    pageState,
    probeConfig: probeMutation.probeConfig,
    isProbePending: probeMutation.isPendingForConfig,
    refreshQuota: quotaMutation.refreshQuota,
    isQuotaRefreshPending: quotaMutation.isPendingForConfig,
    refetchAll: async () => {
      await Promise.all([
        catalogQuery.refetch(),
        settingsQuery.refetch(),
        probeSnapshotsQuery.refetch(),
        usageSummaryQuery.refetch(),
        quotaSnapshotsQuery.refetch(),
        routesQuery.refetch(),
        routeOptionsQuery.refetch(),
      ])
    },
  }
}

export function useProbeProviderConfig() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const pendingConfigIdsRef = useRef(new Set<string>())

  const mutation = useMutation({
    mutationFn: async ({ configId, timeoutMs }: { configId: string; timeoutMs?: number }) => {
      if (pendingConfigIdsRef.current.has(configId)) {
        throw new ModelSettingsMutationBlockedError(configId, 'probe')
      }

      pendingConfigIdsRef.current.add(configId)
      return probeProviderConfig({ configId, timeoutMs }, commandClient)
    },
    onSettled: (_data, _error, variables) => {
      pendingConfigIdsRef.current.delete(variables.configId)
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.probeSnapshots() })
    },
  })

  const probeConfig = useCallback(
    (configId: string, timeoutMs?: number) => {
      if (
        pendingConfigIdsRef.current.has(configId) ||
        (mutation.isPending && mutation.variables?.configId === configId)
      ) {
        return Promise.reject(new ModelSettingsMutationBlockedError(configId, 'probe'))
      }

      return mutation.mutateAsync({ configId, timeoutMs })
    },
    [mutation],
  )

  const isPendingForConfig = useCallback(
    (configId: string) =>
      pendingConfigIdsRef.current.has(configId) ||
      (mutation.isPending && mutation.variables?.configId === configId),
    [mutation.isPending, mutation.variables?.configId],
  )

  return {
    probeConfig,
    isPendingForConfig,
  }
}

export function useRefreshOfficialQuota() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const pendingConfigIdsRef = useRef(new Set<string>())

  const mutation = useMutation({
    mutationFn: async ({ configId }: { configId: string }) => {
      if (pendingConfigIdsRef.current.has(configId)) {
        throw new ModelSettingsMutationBlockedError(configId, 'quota')
      }

      pendingConfigIdsRef.current.add(configId)
      return refreshOfficialQuota({ configId }, commandClient)
    },
    onSettled: (_data, _error, variables) => {
      pendingConfigIdsRef.current.delete(variables.configId)
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.quotaSnapshots() })
    },
  })

  const refreshQuota = useCallback(
    (configId: string) => {
      if (
        pendingConfigIdsRef.current.has(configId) ||
        (mutation.isPending && mutation.variables?.configId === configId)
      ) {
        return Promise.reject(new ModelSettingsMutationBlockedError(configId, 'quota'))
      }

      return mutation.mutateAsync({ configId })
    },
    [mutation],
  )

  const isPendingForConfig = useCallback(
    (configId: string) =>
      pendingConfigIdsRef.current.has(configId) ||
      (mutation.isPending && mutation.variables?.configId === configId),
    [mutation.isPending, mutation.variables?.configId],
  )

  return {
    refreshQuota,
    isPendingForConfig,
  }
}
