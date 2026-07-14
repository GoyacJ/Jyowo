import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useRef, useState } from 'react'

import { providerSettingsQueryKey } from '@/shared/state/workspace-scope'
import {
  type DeleteProviderCapabilityRouteRequest,
  deleteProviderCapabilityRoute,
  getModelSettingsPage,
  type ModelSettingsPageResponse,
  type ProviderSettingsRequest,
  probeProviderConfig,
  refreshModelProviderCatalog,
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
  page: () => [...modelSettingsQueryKeys.all, 'page'] as const,
}

function useModelSettingsPageQuery() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: modelSettingsQueryKeys.page(),
    queryFn: () => getModelSettingsPage(commandClient),
    refetchInterval: (query) =>
      query.state.data?.usageSummary.status === 'rebuilding' ? 250 : false,
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

function fromPageSlice<T>(
  slice: ModelSettingsPageResponse[keyof Pick<
    ModelSettingsPageResponse,
    | 'probeSnapshots'
    | 'usageSummary'
    | 'quotaSnapshots'
    | 'capabilityRoutes'
    | 'capabilityRouteOptions'
  >],
): QuerySlice<T> {
  if (slice.status === 'ready') {
    return { status: 'ready', data: slice.data as T }
  }
  if (slice.status === 'error') {
    return { status: 'error', safeMessage: slice.safeMessage }
  }
  return { status: 'loading' }
}

function inputsFromPageQuery(
  query: ReturnType<typeof useModelSettingsPageQuery>,
): ModelSettingsQueryInputs {
  const page = toQuerySlice(query)
  if (page.status !== 'ready') {
    return {
      catalog: page,
      providerSettings: page,
      probeSnapshots: page,
      usageSummary: page,
      quotaSnapshots: page,
      routes: page,
      routeOptions: page,
    }
  }

  return {
    catalog: { status: 'ready', data: page.data.catalog },
    providerSettings: { status: 'ready', data: page.data.providerSettings },
    probeSnapshots: fromPageSlice(page.data.probeSnapshots),
    usageSummary: fromPageSlice(page.data.usageSummary),
    quotaSnapshots: fromPageSlice(page.data.quotaSnapshots),
    routes: fromPageSlice(page.data.capabilityRoutes),
    routeOptions: fromPageSlice(page.data.capabilityRouteOptions),
  }
}

export function useModelSettingsViewModel() {
  const queryClient = useQueryClient()
  const pageQuery = useModelSettingsPageQuery()
  const probeMutation = useProbeProviderConfig()
  const catalogMutation = useRefreshModelProviderCatalog()
  const quotaMutation = useRefreshOfficialQuota()
  const routeMutation = useCapabilityRouteMutations()
  const defaultMutation = useSetDefaultProviderConfig()

  const inputs = inputsFromPageQuery(pageQuery)

  const pageState: ModelSettingsPageState = buildModelSettingsPageState(inputs)

  return {
    pageState,
    refreshCatalog: catalogMutation.refreshCatalog,
    isCatalogRefreshPending: catalogMutation.isPending,
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
      await Promise.all([
        pageQuery.refetch(),
        queryClient.invalidateQueries({ queryKey: providerSettingsQueryKey }),
      ])
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
      modelOptions?: ProviderSettingsRequest['modelOptions']
      providerId: string
      protocol?: ProviderSettingsRequest['protocol']
    }) => {
      const payload: ProviderSettingsRequest = {
        configId: request.configId,
        displayName: request.displayName,
        modelId: request.modelId,
        modelOptions: request.modelOptions,
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
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() }),
        queryClient.invalidateQueries({ queryKey: providerSettingsQueryKey }),
      ])
    },
  })

  return {
    setDefaultConfig: async (request: {
      baseUrl?: string
      configId: string
      displayName: string
      modelId: string
      providerDefaults?: ProviderSettingsRequest['providerDefaults']
      modelOptions?: ProviderSettingsRequest['modelOptions']
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
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() })
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (request: DeleteProviderCapabilityRouteRequest) =>
      deleteProviderCapabilityRoute(request, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() })
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
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() })
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

function useRefreshModelProviderCatalog() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => refreshModelProviderCatalog(commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() })
    },
  })

  return {
    refreshCatalog: () => mutation.mutateAsync(),
    isPending: mutation.isPending,
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
      await queryClient.invalidateQueries({ queryKey: modelSettingsQueryKeys.page() })
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
