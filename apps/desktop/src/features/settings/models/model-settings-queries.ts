import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useRef, useState } from 'react'

import {
  type DeleteProviderCapabilityRouteRequest,
  deleteProviderCapabilityRoute,
  getModelUsageSummary,
  listModelProviderCatalog,
  listOfficialQuotaSnapshots,
  listProviderCapabilityRouteOptions,
  listProviderCapabilityRoutes,
  listProviderProbeSnapshots,
  listProviderSettings,
  type ProviderSettingsRequest,
  probeProviderConfig,
  refreshOfficialQuota,
  type SaveProviderCapabilityRouteRequest,
  saveProviderCapabilityRoute,
  saveProviderSettings,
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
  readonly operation: 'default' | 'probe' | 'quota'

  constructor(configId: string, operation: 'default' | 'probe' | 'quota') {
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

function useProviderProbeSnapshots(enabled: boolean) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
    queryKey: modelSettingsQueryKeys.probeSnapshots(),
    queryFn: () => listProviderProbeSnapshots(commandClient),
  })
}

function useModelUsageSummary(enabled: boolean) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
    queryKey: modelSettingsQueryKeys.usageSummary(),
    queryFn: () => getModelUsageSummary(commandClient),
  })
}

function useOfficialQuotaSnapshots(enabled: boolean) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
    queryKey: modelSettingsQueryKeys.quotaSnapshots(),
    queryFn: () => listOfficialQuotaSnapshots(commandClient),
  })
}

function useProviderCapabilityRoutes(enabled: boolean) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
    queryKey: modelSettingsQueryKeys.capabilityRoutes(),
    queryFn: () => listProviderCapabilityRoutes(commandClient),
  })
}

function useProviderCapabilityRouteOptions(enabled: boolean) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled,
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
  const criticalQueriesReady = catalogQuery.isSuccess && settingsQuery.isSuccess
  const probeSnapshotsQuery = useProviderProbeSnapshots(criticalQueriesReady)
  const usageSummaryQuery = useModelUsageSummary(criticalQueriesReady)
  const quotaSnapshotsQuery = useOfficialQuotaSnapshots(criticalQueriesReady)
  const routesQuery = useProviderCapabilityRoutes(criticalQueriesReady)
  const routeOptionsQuery = useProviderCapabilityRouteOptions(criticalQueriesReady)
  const probeMutation = useProbeProviderConfig()
  const quotaMutation = useRefreshOfficialQuota()
  const routeMutation = useCapabilityRouteMutations()
  const defaultMutation = useSetDefaultProviderConfig()

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
    saveCapabilityRoute: routeMutation.saveCapabilityRoute,
    deleteCapabilityRoute: routeMutation.deleteCapabilityRoute,
    setDefaultConfig: defaultMutation.setDefaultConfig,
    isSetDefaultPending: defaultMutation.isPendingForConfig,
    isAnySetDefaultPending: defaultMutation.isPending,
    refetchAll: async () => {
      const refetches: Array<Promise<unknown>> = [catalogQuery.refetch(), settingsQuery.refetch()]
      if (criticalQueriesReady) {
        refetches.push(
          probeSnapshotsQuery.refetch(),
          usageSummaryQuery.refetch(),
          quotaSnapshotsQuery.refetch(),
          routesQuery.refetch(),
          routeOptionsQuery.refetch(),
        )
      }
      await Promise.all(refetches)
    },
  }
}

function useSetDefaultProviderConfig() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [pendingConfigId, setPendingConfigId] = useState<string | null>(null)
  const pendingConfigIdRef = useRef<string | null>(null)

  const mutation = useMutation({
    mutationFn: (request: {
      baseUrl?: string
      configId: string
      displayName: string
      modelId: string
      providerDefaults?: ProviderSettingsRequest['providerDefaults']
      providerId: string
      protocol?: ProviderSettingsRequest['protocol']
    }) => {
      const payload: ProviderSettingsRequest = {
        configId: request.configId,
        displayName: request.displayName,
        modelId: request.modelId,
        providerId: request.providerId,
        setDefault: true,
      }
      if (request.baseUrl) {
        payload.baseUrl = request.baseUrl
      }
      if (request.protocol) {
        payload.protocol = request.protocol
      }
      if (request.providerDefaults) {
        payload.providerDefaults = request.providerDefaults
      }

      return saveProviderSettings(payload, commandClient)
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.providerSettings() })
    },
  })

  return {
    setDefaultConfig: async (request: {
      baseUrl?: string
      configId: string
      displayName: string
      modelId: string
      providerDefaults?: ProviderSettingsRequest['providerDefaults']
      providerId: string
      protocol?: ProviderSettingsRequest['protocol']
    }) => {
      if (pendingConfigIdRef.current !== null) {
        throw new ModelSettingsMutationBlockedError(request.configId, 'default')
      }
      pendingConfigIdRef.current = request.configId
      setPendingConfigId(request.configId)
      try {
        return await mutation.mutateAsync(request)
      } finally {
        if (pendingConfigIdRef.current === request.configId) {
          pendingConfigIdRef.current = null
          setPendingConfigId(null)
        }
      }
    },
    isPendingForConfig: (configId: string) => mutation.isPending && pendingConfigId === configId,
    isPending: mutation.isPending || pendingConfigId !== null,
  }
}

function useCapabilityRouteMutations() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  const saveMutation = useMutation({
    mutationFn: (request: SaveProviderCapabilityRouteRequest) =>
      saveProviderCapabilityRoute(request, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.capabilityRoutes() })
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (request: DeleteProviderCapabilityRouteRequest) =>
      deleteProviderCapabilityRoute(request, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.capabilityRoutes() })
    },
  })

  return {
    saveCapabilityRoute: (request: SaveProviderCapabilityRouteRequest) =>
      saveMutation.mutateAsync(request),
    deleteCapabilityRoute: (request: DeleteProviderCapabilityRouteRequest) =>
      deleteMutation.mutateAsync(request),
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
