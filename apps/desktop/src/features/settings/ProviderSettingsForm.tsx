import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { CheckCircle, Eye, Plus, Save, Star } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type UseFormSetValue, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import {
  getWorkspaceScopeGeneration,
  providerSettingsQueryKey,
  providerSettingsSaveMutationKey,
} from '@/shared/state/workspace-scope'
import type {
  ListProviderCapabilityRouteOptionsResponse,
  ListProviderCapabilityRoutesResponse,
  ListProviderSettingsResponse,
  ModelProviderCatalogResponse,
  ProviderConfig,
  ProviderSettingsRequest,
  SaveProviderCapabilityRouteRequest,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/shared/ui/dialog'
import { Toast } from '@/shared/ui/toast'

type ProviderId = ProviderSettingsRequest['providerId']
type ProviderCatalogEntry = ModelProviderCatalogResponse['providers'][number]
type ModelCatalogEntry = ProviderCatalogEntry['models'][number]

type ProviderSettingsFormValues = {
  apiKey: string
  baseUrl: string
  configId: string
  displayName: string
  modelId: string
  providerId: ProviderId
}

type ProviderToast = {
  description?: string
  id: number
  title: string
  variant: 'destructive' | 'success'
}

const MINIMAX_BASE_URLS = {
  china: 'https://api.minimaxi.com',
  international: 'https://api.minimax.io',
} as const
const SAVED_API_KEY_MASK = '\u2022'.repeat(32)
const modelProviderCatalogQueryKey = ['model-provider-catalog'] as const
const providerCapabilityRoutesQueryKey = ['provider-capability-routes'] as const
const providerCapabilityRouteOptionsQueryKey = ['provider-capability-route-options'] as const

type ProviderCapabilityRouteOption = ListProviderCapabilityRouteOptionsResponse['options'][number]
type ProviderCapabilityRoute = ListProviderCapabilityRoutesResponse['routes'][number]
type CapabilityRouteKind = ProviderCapabilityRouteOption['kind']

const capabilityRouteKindOrder = [
  'image_generation',
  'video_generation',
  'text_to_speech',
  'speech_to_text',
  'music_generation',
] as const satisfies readonly CapabilityRouteKind[]

const capabilityRouteKindLabelKeys = {
  image_generation: 'provider.capabilityRouting.kind.imageGeneration',
  video_generation: 'provider.capabilityRouting.kind.videoGeneration',
  text_to_speech: 'provider.capabilityRouting.kind.textToSpeech',
  speech_to_text: 'provider.capabilityRouting.kind.speechToText',
  music_generation: 'provider.capabilityRouting.kind.musicGeneration',
} as const satisfies Record<CapabilityRouteKind, string>

type ProviderSettingsSaveMutationVariables = {
  requestId: string
  workspaceScopeGeneration: number
}

const modelCapabilities = [
  ['toolCalling', 'provider.capability.tools'],
  ['vision', 'provider.capability.vision'],
  ['videoInput', 'provider.capability.videoInput'],
  ['reasoning', 'provider.capability.thinking'],
  ['streaming', 'provider.capability.streaming'],
  ['structuredOutput', 'provider.capability.structuredOutput'],
  ['promptCache', 'provider.capability.promptCache'],
] as const

export function ProviderSettingsForm() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const catalogQuery = useQuery({
    queryKey: modelProviderCatalogQueryKey,
    queryFn: () => commandClient.listModelProviderCatalog(),
  })
  const settingsQuery = useQuery({
    queryKey: providerSettingsQueryKey,
    queryFn: () => commandClient.listProviderSettings(),
  })
  const routeOptionsQuery = useQuery({
    queryKey: providerCapabilityRouteOptionsQueryKey,
    queryFn: () => commandClient.listProviderCapabilityRouteOptions(),
  })
  const routesQuery = useQuery({
    queryKey: providerCapabilityRoutesQueryKey,
    queryFn: () => commandClient.listProviderCapabilityRoutes(),
  })
  const [formError, setFormError] = useState<string | null>(null)
  const [selectedConfigId, setSelectedConfigId] = useState<string | null>(null)
  const [isSettingDefault, setIsSettingDefault] = useState(false)
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false)
  const [revealedApiKey, setRevealedApiKey] = useState<{
    configId: string
    value: string
  } | null>(null)
  const [checkingConfigIds, setCheckingConfigIds] = useState<Record<string, true>>({})
  const [toast, setToast] = useState<ProviderToast | null>(null)
  const selectedConfigIdRef = useRef<string | null>(null)
  const saveRequestIdRef = useRef(0)
  const pendingSaveRequestsRef = useRef(new Map<string, ProviderSettingsRequest>())
  const revealVersionRef = useRef(0)
  const catalog = catalogQuery.data?.providers ?? []
  const profiles = settingsQuery.data?.configs ?? []
  const loadError = catalogQuery.error
    ? getCommandErrorMessage(catalogQuery.error)
    : settingsQuery.error
      ? getCommandErrorMessage(settingsQuery.error)
      : null
  const routeLoadError = routeOptionsQuery.error
    ? getCommandErrorMessage(routeOptionsQuery.error)
    : routesQuery.error
      ? getCommandErrorMessage(routesQuery.error)
      : null
  const isLoading = catalogQuery.isLoading || settingsQuery.isLoading
  const isRouteLoading = routeOptionsQuery.isLoading || routesQuery.isLoading
  const isRouteReady = routeOptionsQuery.isSuccess && routesQuery.isSuccess
  const saveSettingsMutation = useMutation({
    mutationKey: providerSettingsSaveMutationKey,
    mutationFn: async ({ requestId }: ProviderSettingsSaveMutationVariables) => {
      const request = pendingSaveRequestsRef.current.get(requestId)
      if (!request) {
        throw new Error('provider settings save request missing')
      }
      return commandClient.saveProviderSettings(request)
    },
    onSuccess: (saved, variables) => {
      if (variables.workspaceScopeGeneration !== getWorkspaceScopeGeneration(queryClient)) {
        return
      }
      cacheSavedProfile(saved.config)
    },
    onSettled: (_data, _error, variables) => {
      pendingSaveRequestsRef.current.delete(variables.requestId)
    },
  })
  const checkProviderSettingsMutation = useMutation({
    mutationFn: (request: { modelId: string; providerId: ProviderId }) =>
      commandClient.validateProviderSettings(request),
  })
  const revealApiKeyMutation = useMutation({
    mutationFn: async (request: { configId: string; revealVersion: number }) => {
      const { configId, revealVersion } = request
      const reveal = await commandClient.requestProviderConfigApiKeyReveal(configId)
      const payload = await commandClient.getProviderConfigApiKey(configId, reveal.revealToken)
      if (selectedConfigIdRef.current === configId && revealVersionRef.current === revealVersion) {
        setRevealedApiKey({
          configId,
          value: payload.apiKey,
        })
      }
    },
  })
  const saveCapabilityRouteMutation = useMutation({
    mutationFn: (request: SaveProviderCapabilityRouteRequest) =>
      commandClient.saveProviderCapabilityRoute(request),
    onSuccess: (saved) => {
      queryClient.setQueryData<ListProviderCapabilityRoutesResponse>(
        providerCapabilityRoutesQueryKey,
        {
          version: saved.version,
          routes: saved.routes,
        },
      )
    },
  })
  const deleteCapabilityRouteMutation = useMutation({
    mutationFn: (request: Parameters<typeof commandClient.deleteProviderCapabilityRoute>[0]) =>
      commandClient.deleteProviderCapabilityRoute(request),
    onSuccess: (deleted) => {
      queryClient.setQueryData<ListProviderCapabilityRoutesResponse>(
        providerCapabilityRoutesQueryKey,
        {
          version: deleted.version,
          routes: deleted.routes,
        },
      )
    },
  })
  const {
    clearErrors,
    formState: { errors, isSubmitting },
    handleSubmit,
    register,
    reset,
    setError,
    setValue,
    watch,
  } = useForm<ProviderSettingsFormValues>({
    defaultValues: {
      apiKey: '',
      baseUrl: '',
      configId: '',
      displayName: '',
      modelId: '',
      providerId: 'openai',
    },
  })
  const {
    clearErrors: clearEditErrors,
    formState: { errors: editErrors, isSubmitting: isEditSubmitting },
    handleSubmit: handleEditSubmit,
    register: registerEdit,
    reset: resetEdit,
    setError: setEditError,
    setValue: setEditValue,
    watch: watchEdit,
  } = useForm<ProviderSettingsFormValues>({
    defaultValues: {
      apiKey: '',
      baseUrl: '',
      configId: '',
      displayName: '',
      modelId: '',
      providerId: 'openai',
    },
  })

  const selectedProviderId = watch('providerId')
  const editProviderId = watchEdit('providerId')
  const editApiKey = watchEdit('apiKey')
  const selectedProfile = useMemo(
    () => profiles.find((profile) => profile.id === selectedConfigId) ?? null,
    [profiles, selectedConfigId],
  )
  const selectedProvider = useMemo(
    () => catalog.find((provider) => provider.providerId === selectedProviderId),
    [catalog, selectedProviderId],
  )
  const selectedEditProvider = useMemo(
    () => catalog.find((provider) => provider.providerId === editProviderId),
    [catalog, editProviderId],
  )
  const selectedProfileProvider = useMemo(
    () => catalog.find((provider) => provider.providerId === selectedProfile?.providerId),
    [catalog, selectedProfile],
  )
  const selectedProfileServiceCapabilities = selectedProfileProvider?.serviceCapabilities ?? []
  const eligibleRouteOptions = useMemo(
    () => (routeOptionsQuery.data?.options ?? []).filter((option) => option.runtimeSupported),
    [routeOptionsQuery.data],
  )
  const routeOptionsByKind = useMemo(() => {
    const groups = new Map<CapabilityRouteKind, ProviderCapabilityRouteOption[]>()
    for (const option of eligibleRouteOptions) {
      const current = groups.get(option.kind) ?? []
      current.push(option)
      groups.set(option.kind, current)
    }
    return groups
  }, [eligibleRouteOptions])
  const savedCapabilityRoutes = routesQuery.data?.routes ?? []
  const defaultMainModelProfile = useMemo(() => {
    const defaultConfigId = settingsQuery.data?.defaultConfigId
    if (!defaultConfigId) {
      return null
    }
    return profiles.find((profile) => profile.id === defaultConfigId) ?? null
  }, [profiles, settingsQuery.data?.defaultConfigId])
  const defaultMainModelCapability = useMemo(() => {
    if (!defaultMainModelProfile) {
      return null
    }
    return (
      defaultMainModelProfile.modelDescriptor?.conversationCapability ??
      catalog
        .find((provider) => provider.providerId === defaultMainModelProfile.providerId)
        ?.models.find((model) => model.modelId === defaultMainModelProfile.modelId)
        ?.conversationCapability ??
      null
    )
  }, [catalog, defaultMainModelProfile])
  const showToolCallingWarning = defaultMainModelCapability?.toolCalling === false && isRouteReady
  const isRouteMutationPending =
    saveCapabilityRouteMutation.isPending || deleteCapabilityRouteMutation.isPending
  const selectedProfileModel = useMemo(
    () =>
      selectedProfile?.modelDescriptor ??
      selectedProfileProvider?.models.find((model) => model.modelId === selectedProfile?.modelId),
    [selectedProfile, selectedProfileProvider],
  )
  const selectedRunnableModels = useMemo(
    () => runnableModels(selectedProvider, profiles),
    [profiles, selectedProvider],
  )
  const selectedEditRunnableModels = useMemo(() => {
    const models = runnableModels(selectedEditProvider, profiles)
    if (
      selectedProfile?.providerId === editProviderId &&
      selectedProfileModel &&
      !models.some((model) => model.modelId === selectedProfileModel.modelId)
    ) {
      return [...models, selectedProfileModel]
    }
    return models
  }, [editProviderId, profiles, selectedEditProvider, selectedProfile, selectedProfileModel])
  const selectedRevealedApiKey =
    selectedConfigId && revealedApiKey?.configId === selectedConfigId ? revealedApiKey.value : null
  const isSelectedApiKeyRevealing =
    revealApiKeyMutation.isPending && selectedConfigId === revealApiKeyMutation.variables?.configId
  const isSelectedProfileChecking = selectedConfigId
    ? checkingConfigIds[selectedConfigId] === true
    : false
  const shouldShowSavedApiKeyMask = selectedProfile?.hasApiKey === true && editApiKey.length === 0
  selectedConfigIdRef.current = selectedConfigId

  const clearRevealedApiKey = useCallback(() => {
    revealVersionRef.current += 1
    setRevealedApiKey(null)
  }, [])

  function clearEditApiKey() {
    setEditValue('apiKey', '')
  }

  function resetCreateForm(provider: ProviderCatalogEntry | undefined = catalog[0]) {
    if (provider) {
      reset(createFormValuesFromProvider(provider))
    } else {
      reset({
        apiKey: '',
        baseUrl: '',
        configId: '',
        displayName: '',
        modelId: '',
        providerId: 'openai',
      })
    }
    clearErrors()
    setFormError(null)
  }

  function handleCreateDialogOpenChange(open: boolean) {
    setIsCreateDialogOpen(open)
    clearRevealedApiKey()
    if (open) {
      resetCreateForm()
      clearEditApiKey()
    }
  }

  function cacheSavedProfile(savedConfig: ProviderConfig) {
    queryClient.setQueryData<ListProviderSettingsResponse>(providerSettingsQueryKey, (current) => {
      const currentConfigs = current?.configs ?? profiles
      const nextConfigs = currentConfigs.filter((profile) => profile.id !== savedConfig.id)
      nextConfigs.push(savedConfig)
      return {
        defaultConfigId: savedConfig.isDefault
          ? savedConfig.id
          : (current?.defaultConfigId ?? settingsQuery.data?.defaultConfigId ?? null),
        configs: nextConfigs
          .map((profile) =>
            savedConfig.isDefault
              ? {
                  ...profile,
                  isDefault: profile.id === savedConfig.id,
                }
              : profile,
          )
          .sort((left, right) => left.id.localeCompare(right.id)),
      }
    })
  }

  async function saveProviderSettingsRequest(request: ProviderSettingsRequest) {
    saveRequestIdRef.current += 1
    const requestId = String(saveRequestIdRef.current)
    const workspaceScopeGeneration = getWorkspaceScopeGeneration(queryClient)
    pendingSaveRequestsRef.current.set(requestId, request)
    const saved = await saveSettingsMutation.mutateAsync({
      requestId,
      workspaceScopeGeneration,
    })
    if (workspaceScopeGeneration !== getWorkspaceScopeGeneration(queryClient)) {
      return null
    }
    return saved
  }

  useEffect(() => {
    return () => {
      pendingSaveRequestsRef.current.clear()
    }
  }, [])

  useEffect(() => {
    const configs = settingsQuery.data?.configs ?? []
    if (configs.length === 0) {
      setSelectedConfigId(null)
      return
    }

    setSelectedConfigId((currentConfigId) => {
      if (currentConfigId && configs.some((profile) => profile.id === currentConfigId)) {
        return currentConfigId
      }
      return (
        configs.find((profile) => profile.id === settingsQuery.data?.defaultConfigId)?.id ??
        configs[0]?.id ??
        null
      )
    })
  }, [settingsQuery.data])

  useEffect(() => {
    if (profiles.length === 0 && catalog[0]) {
      reset(createFormValuesFromProvider(catalog[0]))
    }
  }, [catalog, profiles.length, reset])

  useEffect(() => {
    clearRevealedApiKey()
  }, [clearRevealedApiKey, selectedConfigId])

  useEffect(() => {
    if (!selectedProfile) {
      resetEdit({
        apiKey: '',
        baseUrl: '',
        configId: '',
        displayName: '',
        modelId: '',
        providerId: 'openai',
      })
      return
    }

    resetEdit(createFormValuesFromProfile(selectedProfile))
    clearEditErrors()
  }, [
    selectedProfile?.baseUrl,
    selectedProfile?.displayName,
    selectedProfile?.id,
    selectedProfile?.modelId,
    selectedProfile?.providerId,
    selectedProfileProvider,
  ])

  async function submit(values: ProviderSettingsFormValues) {
    setFormError(null)
    clearEditApiKey()
    clearRevealedApiKey()

    const provider = catalog.find(
      (currentProvider) => currentProvider.providerId === values.providerId,
    )
    const model = runnableModels(provider, profiles).find(
      (currentModel) => currentModel.modelId === values.modelId,
    )
    const trimmedApiKey = values.apiKey.trim()
    let hasValidationError = false

    if (!provider) {
      setError('providerId', {
        message: t('provider.errors.providerRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (!model) {
      setError('modelId', {
        message: t('provider.errors.modelRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (!trimmedApiKey) {
      setError('apiKey', {
        message: t('provider.errors.apiKeyRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (hasValidationError || !provider || !model) {
      return
    }

    setValue('apiKey', '')

    const request: ProviderSettingsRequest = {
      baseUrl: optionalTrimmed(values.baseUrl),
      displayName: optionalTrimmed(values.displayName),
      modelId: model.modelId,
      providerId: provider.providerId,
      setDefault: true,
    }

    if (trimmedApiKey) {
      request.apiKey = trimmedApiKey
    }

    try {
      const saved = await saveProviderSettingsRequest(request)
      if (!saved) {
        return
      }
      setSelectedConfigId(saved.config.id)
      setIsCreateDialogOpen(false)
      reset(createFormValuesFromProvider(provider))
    } catch (error) {
      setFormError(getCommandErrorMessage(error))
    }
  }

  async function saveSelectedProfile(values: ProviderSettingsFormValues) {
    if (!selectedProfile) {
      return
    }

    setFormError(null)
    clearEditApiKey()
    clearRevealedApiKey()

    const providerId = values.providerId || selectedProfile.providerId
    const modelId = values.modelId || selectedProfile.modelId
    const provider =
      catalog.find((currentProvider) => currentProvider.providerId === providerId) ??
      selectedProfileProvider
    const models = runnableModels(provider, profiles)
    const model =
      models.find((currentModel) => currentModel.modelId === modelId) ??
      (selectedProfileModel?.modelId === modelId ? selectedProfileModel : undefined)
    const trimmedApiKey = values.apiKey.trim()
    let hasValidationError = false

    if (!provider) {
      setEditError('providerId', {
        message: t('provider.errors.providerRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (!model) {
      setEditError('modelId', {
        message: t('provider.errors.modelRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (!selectedProfile.hasApiKey && !trimmedApiKey) {
      setEditError('apiKey', {
        message: t('provider.errors.apiKeyRequired'),
        type: 'manual',
      })
      hasValidationError = true
    }

    if (hasValidationError || !provider || !model) {
      return
    }

    setEditValue('apiKey', '')

    const request: ProviderSettingsRequest = {
      configId: selectedProfile.id,
      baseUrl: optionalTrimmed(values.baseUrl),
      displayName: optionalTrimmed(values.displayName),
      modelId: model.modelId,
      providerId: provider.providerId,
      setDefault: selectedProfile.isDefault,
    }

    if (trimmedApiKey) {
      request.apiKey = trimmedApiKey
    }

    try {
      const saved = await saveProviderSettingsRequest(request)
      if (!saved) {
        return
      }
      setSelectedConfigId(saved.config.id)
      resetEdit(createFormValuesFromProfile(saved.config))
    } catch (error) {
      setFormError(getCommandErrorMessage(error))
    }
  }

  const setProfileChecking = useCallback((configId: string, isChecking: boolean) => {
    setCheckingConfigIds((currentCheckingConfigIds) => {
      if (isChecking) {
        return {
          ...currentCheckingConfigIds,
          [configId]: true,
        }
      }

      const { [configId]: _removed, ...nextCheckingConfigIds } = currentCheckingConfigIds
      return nextCheckingConfigIds
    })
  }, [])

  const checkProviderConfiguration = useCallback(
    async (profile: ProviderConfig | null) => {
      if (!profile) {
        return
      }

      setFormError(null)

      setProfileChecking(profile.id, true)
      try {
        await checkProviderSettingsMutation.mutateAsync({
          modelId: profile.modelId,
          providerId: profile.providerId,
        })
        setToast({
          description: t('provider.testSuccessToastDescription'),
          id: Date.now(),
          title: t('provider.testSuccessToastTitle'),
          variant: 'success',
        })
      } catch (error) {
        setToast({
          description: getCommandErrorMessage(error),
          id: Date.now(),
          title: t('provider.testErrorToastTitle'),
          variant: 'destructive',
        })
      } finally {
        setProfileChecking(profile.id, false)
      }
    },
    [checkProviderSettingsMutation, setProfileChecking, t],
  )

  async function revealSelectedApiKey() {
    if (!selectedProfile) {
      return
    }

    setFormError(null)
    clearRevealedApiKey()
    const configId = selectedProfile.id
    const revealVersion = revealVersionRef.current

    try {
      await revealApiKeyMutation.mutateAsync({ configId, revealVersion })
    } catch (error) {
      setFormError(getCommandErrorMessage(error))
    }
  }

  async function setSelectedProfileAsDefault() {
    if (!selectedProfile) {
      return
    }

    setFormError(null)
    clearEditApiKey()
    clearRevealedApiKey()
    setIsSettingDefault(true)

    const request: ProviderSettingsRequest = {
      configId: selectedProfile.id,
      displayName: selectedProfile.displayName,
      modelId: selectedProfile.modelId,
      providerId: selectedProfile.providerId,
      setDefault: true,
    }
    if (selectedProfile.baseUrl) {
      request.baseUrl = selectedProfile.baseUrl
    }

    try {
      const saved = await saveProviderSettingsRequest(request)
      if (!saved) {
        return
      }
      setSelectedConfigId(saved.config.id)
    } catch (error) {
      setFormError(getCommandErrorMessage(error))
    } finally {
      setIsSettingDefault(false)
    }
  }

  return (
    <div className="space-y-4">
      {toast ? (
        <Toast
          closeLabel={t('provider.closeToast')}
          description={toast.description}
          key={toast.id}
          onClose={() => setToast(null)}
          title={toast.title}
          variant={toast.variant}
        />
      ) : null}

      {loadError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {loadError}
        </div>
      ) : null}

      <div className="grid min-h-[560px] gap-4 lg:grid-cols-[minmax(240px,300px)_minmax(0,1fr)]">
        <section
          aria-label={t('provider.savedProfiles')}
          className="flex min-h-0 flex-col rounded-md border border-border bg-background"
        >
          <div className="flex items-center justify-between gap-3 border-border border-b px-3 py-2">
            <div className="text-muted-foreground text-sm">{t('provider.savedProfiles')}</div>
            <Dialog open={isCreateDialogOpen} onOpenChange={handleCreateDialogOpenChange}>
              <DialogTrigger asChild>
                <Button disabled={isLoading || catalog.length === 0} size="sm" type="button">
                  <Plus className="size-4" />
                  {t('provider.newConfig')}
                </Button>
              </DialogTrigger>
              <DialogContent className="w-[min(calc(100vw-2rem),42rem)]">
                <DialogHeader>
                  <DialogTitle>{t('provider.createTitle')}</DialogTitle>
                  <DialogDescription>{t('provider.createDescription')}</DialogDescription>
                </DialogHeader>

                <form className="space-y-5" onSubmit={handleSubmit(submit)}>
                  <div className="grid gap-4 md:grid-cols-2">
                    <label className="space-y-2 text-sm">
                      <span className="font-medium">{t('provider.profileName')}</span>
                      <input
                        className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        disabled={isSubmitting || isLoading}
                        placeholder={selectedProvider?.displayName ?? 'OpenAI'}
                        {...register('displayName')}
                      />
                    </label>

                    <label className="space-y-2 text-sm">
                      <span className="font-medium">{t('provider.provider')}</span>
                      <select
                        className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        disabled={isSubmitting || isLoading}
                        {...register('providerId', {
                          onChange: (event) => {
                            const provider = catalog.find(
                              (currentProvider) =>
                                currentProvider.providerId === event.target.value,
                            )
                            if (provider) {
                              setFormFromProvider(provider, setValue)
                            }
                          },
                        })}
                      >
                        {catalog.map((provider) => (
                          <option key={provider.providerId} value={provider.providerId}>
                            {provider.displayName}
                          </option>
                        ))}
                      </select>
                      {errors.providerId ? (
                        <span className="block text-destructive text-xs">
                          {errors.providerId.message}
                        </span>
                      ) : null}
                    </label>
                  </div>

                  <div className="grid gap-4 md:grid-cols-2">
                    <label className="space-y-2 text-sm">
                      <span className="font-medium">{t('provider.model')}</span>
                      <select
                        className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        disabled={isSubmitting || isLoading || !selectedProvider}
                        {...register('modelId')}
                      >
                        {selectedRunnableModels.map((model) => (
                          <option key={model.modelId} value={model.modelId}>
                            {model.displayName}
                          </option>
                        ))}
                      </select>
                      {errors.modelId ? (
                        <span className="block text-destructive text-xs">
                          {errors.modelId.message}
                        </span>
                      ) : null}
                    </label>

                    <label className="space-y-2 text-sm">
                      <span className="font-medium">{t('provider.baseUrl')}</span>
                      <input
                        className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        disabled={isSubmitting || isLoading}
                        placeholder={selectedProvider?.defaultBaseUrl}
                        {...register('baseUrl')}
                      />
                    </label>
                  </div>

                  {selectedProvider?.providerId === 'minimax' ? (
                    <div className="rounded-md border border-border bg-surface px-3 py-2">
                      <div className="text-muted-foreground text-xs">
                        {t('provider.baseUrlRegion')}
                      </div>
                      <div className="mt-2 flex flex-wrap gap-2">
                        <Button
                          disabled={isSubmitting || isLoading}
                          onClick={() => setValue('baseUrl', MINIMAX_BASE_URLS.international)}
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          {t('provider.baseUrlInternational')}
                        </Button>
                        <Button
                          disabled={isSubmitting || isLoading}
                          onClick={() => setValue('baseUrl', MINIMAX_BASE_URLS.china)}
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          {t('provider.baseUrlChina')}
                        </Button>
                      </div>
                    </div>
                  ) : null}

                  <label className="block space-y-2 text-sm">
                    <span className="font-medium">{t('provider.apiKey')}</span>
                    <input
                      className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      disabled={isSubmitting || isLoading}
                      placeholder={t('provider.apiKeyPlaceholder')}
                      type="password"
                      {...register('apiKey')}
                    />
                    {errors.apiKey ? (
                      <span className="block text-destructive text-xs">
                        {errors.apiKey.message}
                      </span>
                    ) : null}
                  </label>

                  {formError ? (
                    <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                      {formError}
                    </div>
                  ) : null}

                  <div className="flex flex-wrap justify-end gap-2">
                    <Button
                      disabled={isSubmitting || isLoading || catalog.length === 0}
                      type="submit"
                    >
                      <Save className="size-4" />
                      {isSubmitting ? t('provider.saving') : t('provider.save')}
                    </Button>
                  </div>
                </form>
              </DialogContent>
            </Dialog>
          </div>
          <nav className="min-h-0 flex-1 space-y-2 overflow-y-auto p-2">
            {profiles.length === 0 ? (
              <div className="rounded-md border border-dashed border-border px-4 py-6 text-center text-muted-foreground text-sm">
                {t('provider.emptyProfiles')}
              </div>
            ) : null}
            {profiles.map((profile) => {
              const isProfileChecking = checkingConfigIds[profile.id] === true

              return (
                <button
                  aria-pressed={selectedConfigId === profile.id}
                  className="block w-full rounded-md border border-border bg-surface px-2.5 py-2 text-left text-sm transition-colors hover:bg-muted/45 disabled:cursor-not-allowed disabled:opacity-60 aria-pressed:border-primary aria-pressed:bg-muted/35"
                  disabled={isSubmitting || isLoading}
                  key={profile.id}
                  onClick={() => {
                    setSelectedConfigId(profile.id)
                    clearEditApiKey()
                    clearRevealedApiKey()
                    setFormError(null)
                  }}
                  type="button"
                >
                  <span className="flex items-center justify-between gap-3">
                    <span className="min-w-0">
                      <span className="block truncate font-medium text-foreground">
                        {profile.displayName}
                      </span>
                      <span className="mt-0.5 block truncate text-muted-foreground text-xs">
                        {profile.providerId} / {profile.modelId}
                      </span>
                    </span>
                    <span className="flex shrink-0 flex-col items-end gap-1">
                      {isProfileChecking ? (
                        <Badge variant="outline">{t('provider.testing')}</Badge>
                      ) : null}
                      {profile.isDefault ? (
                        <Badge variant="secondary">{t('provider.default')}</Badge>
                      ) : null}
                    </span>
                  </span>
                </button>
              )
            })}
          </nav>
        </section>

        <section
          aria-label={t('provider.detailsTitle')}
          className="min-h-[420px] rounded-md border border-border bg-background p-5"
        >
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <h2 className="flex min-w-0 flex-wrap items-center gap-2 font-semibold text-base">
                <span className="truncate">
                  {selectedProfile?.displayName ?? t('provider.emptyDetails')}
                </span>
              </h2>
            </div>
            {selectedProfile ? (
              <div className="flex shrink-0 flex-wrap justify-end gap-2">
                {!selectedProfile.isDefault ? (
                  <Button
                    disabled={isSettingDefault || isLoading}
                    onClick={() => void setSelectedProfileAsDefault()}
                    size="sm"
                    type="button"
                    variant="outline"
                  >
                    <Star className="size-4" />
                    {isSettingDefault ? t('provider.settingDefault') : t('provider.setDefault')}
                  </Button>
                ) : null}
                <Button
                  disabled={isSelectedProfileChecking || isLoading}
                  onClick={() => void checkProviderConfiguration(selectedProfile)}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  <CheckCircle className="size-4" />
                  {isSelectedProfileChecking ? t('provider.testing') : t('provider.test')}
                </Button>
              </div>
            ) : null}
          </div>

          {selectedProfile ? (
            <form className="mt-6 space-y-5" onSubmit={handleEditSubmit(saveSelectedProfile)}>
              <div className="grid gap-4 md:grid-cols-2">
                <label className="space-y-2 text-sm">
                  <span className="font-medium">{t('provider.profileName')}</span>
                  <input
                    aria-label={t('provider.profileName')}
                    className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    disabled={isEditSubmitting || isLoading}
                    {...registerEdit('displayName')}
                  />
                </label>

                <label className="space-y-2 text-sm">
                  <span className="font-medium">{t('provider.provider')}</span>
                  <select
                    aria-label={t('provider.provider')}
                    className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    disabled={isEditSubmitting || isLoading}
                    {...registerEdit('providerId', {
                      onChange: (event) => {
                        const provider = catalog.find(
                          (currentProvider) => currentProvider.providerId === event.target.value,
                        )
                        if (provider) {
                          setEditValue('apiKey', '')
                          setEditValue('baseUrl', defaultBaseUrlForProvider(provider))
                          setEditValue(
                            'modelId',
                            runnableModels(provider, profiles)[0]?.modelId ?? '',
                          )
                        }
                      },
                    })}
                  >
                    {catalog.map((provider) => (
                      <option key={provider.providerId} value={provider.providerId}>
                        {provider.displayName}
                      </option>
                    ))}
                  </select>
                  {editErrors.providerId ? (
                    <span className="block text-destructive text-xs">
                      {editErrors.providerId.message}
                    </span>
                  ) : null}
                </label>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <label className="space-y-2 text-sm">
                  <span className="font-medium">{t('provider.model')}</span>
                  <select
                    aria-label={t('provider.model')}
                    className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    disabled={isEditSubmitting || isLoading || !selectedEditProvider}
                    {...registerEdit('modelId')}
                  >
                    {selectedEditRunnableModels.map((model) => (
                      <option key={model.modelId} value={model.modelId}>
                        {model.displayName}
                      </option>
                    ))}
                  </select>
                  {editErrors.modelId ? (
                    <span className="block text-destructive text-xs">
                      {editErrors.modelId.message}
                    </span>
                  ) : null}
                </label>

                <label className="space-y-2 text-sm">
                  <span className="font-medium">{t('provider.baseUrl')}</span>
                  <input
                    aria-label={t('provider.baseUrl')}
                    className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    disabled={isEditSubmitting || isLoading}
                    placeholder={selectedEditProvider?.defaultBaseUrl}
                    {...registerEdit('baseUrl')}
                  />
                </label>
              </div>

              {selectedEditProvider?.providerId === 'minimax' ? (
                <div className="rounded-md border border-border bg-surface px-3 py-2">
                  <div className="text-muted-foreground text-xs">{t('provider.baseUrlRegion')}</div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <Button
                      disabled={isEditSubmitting || isLoading}
                      onClick={() => setEditValue('baseUrl', MINIMAX_BASE_URLS.international)}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      {t('provider.baseUrlInternational')}
                    </Button>
                    <Button
                      disabled={isEditSubmitting || isLoading}
                      onClick={() => setEditValue('baseUrl', MINIMAX_BASE_URLS.china)}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      {t('provider.baseUrlChina')}
                    </Button>
                  </div>
                </div>
              ) : null}

              {selectedProfileModel ? (
                <div className="rounded-md border border-border bg-surface px-3 py-3">
                  <div className="text-muted-foreground text-xs">{t('provider.capabilities')}</div>
                  <div className="mt-3 grid gap-2 md:grid-cols-2">
                    {modelCapabilities.map(([key, labelKey]) => {
                      const capability = selectedProfileModel.conversationCapability
                      const supported =
                        key === 'vision'
                          ? capability.inputModalities.includes('image')
                          : key === 'videoInput'
                            ? capability.inputModalities.includes('video')
                            : capability[key]
                      return (
                        <div
                          className="flex items-center justify-between gap-3 rounded-md border border-border bg-background px-3 py-2 text-sm"
                          key={key}
                        >
                          <span>{t(labelKey)}</span>
                          <Badge variant={supported ? 'success' : 'outline'}>
                            {supported
                              ? t('provider.capability.supported')
                              : t('provider.capability.unsupported')}
                          </Badge>
                        </div>
                      )
                    })}
                  </div>
                </div>
              ) : null}

              {selectedProfileServiceCapabilities.length > 0 ? (
                <div className="rounded-md border border-border bg-surface px-3 py-3">
                  <div className="text-muted-foreground text-xs">
                    {t('provider.serviceCapabilities')}
                  </div>
                  <div className="mt-3 grid gap-2">
                    {selectedProfileServiceCapabilities.map((capability) => (
                      <div
                        className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border bg-background px-3 py-2 text-sm"
                        key={capability.operationId}
                      >
                        <span className="break-all font-medium">{capability.operationId}</span>
                        <Badge variant="outline">
                          {t(`provider.serviceCategory.${capability.category}`)}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              <div className="rounded-md border border-border bg-surface px-3 py-2">
                <label className="block space-y-2 text-sm">
                  <span className="flex items-center justify-between gap-3">
                    <span className="font-medium">{t('provider.apiKey')}</span>
                    <span className="text-muted-foreground text-xs">
                      {selectedProfile.hasApiKey
                        ? t('provider.savedApiKeyAvailable')
                        : t('provider.savedApiKeyMissing')}
                    </span>
                  </span>
                  <div className="relative">
                    <input
                      aria-label={t('provider.apiKey')}
                      className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      disabled={isEditSubmitting || isLoading}
                      placeholder={
                        shouldShowSavedApiKeyMask
                          ? ''
                          : selectedProfile.hasApiKey
                            ? t('provider.apiKeyExistingPlaceholder')
                            : t('provider.apiKeyPlaceholder')
                      }
                      type="password"
                      {...registerEdit('apiKey', {
                        onChange: clearRevealedApiKey,
                      })}
                    />
                    {shouldShowSavedApiKeyMask ? (
                      <div
                        aria-hidden="true"
                        className="pointer-events-none absolute top-1/2 right-3 left-3 -translate-y-1/2 truncate font-mono text-foreground text-sm"
                      >
                        {SAVED_API_KEY_MASK}
                      </div>
                    ) : null}
                  </div>
                  {editErrors.apiKey ? (
                    <span className="block text-destructive text-xs">
                      {editErrors.apiKey.message}
                    </span>
                  ) : null}
                </label>
                {selectedProfile.hasApiKey ? (
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    <Button
                      disabled={isEditSubmitting || isLoading || isSelectedApiKeyRevealing}
                      onClick={() => void revealSelectedApiKey()}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <Eye className="size-4" />
                      {isSelectedApiKeyRevealing
                        ? t('provider.revealingApiKey')
                        : t('provider.revealApiKey')}
                    </Button>
                  </div>
                ) : null}
                {selectedRevealedApiKey ? (
                  <div className="mt-3 rounded-md border border-border bg-background px-3 py-2">
                    <code className="block break-all font-mono text-sm">
                      {selectedRevealedApiKey}
                    </code>
                  </div>
                ) : null}
              </div>

              {formError ? (
                <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
                  {formError}
                </div>
              ) : null}

              <div className="flex flex-wrap justify-end gap-2">
                <Button
                  disabled={isEditSubmitting || isLoading || catalog.length === 0}
                  type="submit"
                >
                  <Save className="size-4" />
                  {isEditSubmitting ? t('provider.saving') : t('provider.save')}
                </Button>
              </div>
            </form>
          ) : (
            <div className="mt-6 rounded-md border border-dashed border-border px-4 py-10 text-center text-muted-foreground text-sm">
              {t('provider.emptyDetails')}
            </div>
          )}
        </section>
      </div>

      <section
        aria-label={t('provider.capabilityRouting.title')}
        className="rounded-md border border-border bg-background p-5"
      >
        <div className="space-y-1">
          <h2 className="font-semibold text-base">{t('provider.capabilityRouting.title')}</h2>
          <p className="text-muted-foreground text-sm">
            {t('provider.capabilityRouting.description')}
          </p>
        </div>

        {routeLoadError ? (
          <div className="mt-4 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
            {routeLoadError}
          </div>
        ) : null}

        {isRouteLoading ? (
          <p className="mt-4 text-muted-foreground text-sm">
            {t('provider.capabilityRouting.loading')}
          </p>
        ) : null}

        {isRouteReady && eligibleRouteOptions.length === 0 ? (
          <p className="mt-4 text-muted-foreground text-sm">
            {t('provider.capabilityRouting.empty')}
          </p>
        ) : null}

        {showToolCallingWarning ? (
          <div className="mt-4 rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2 text-amber-900 text-sm dark:text-amber-100">
            {t('provider.capabilityRouting.toolCallingWarning')}
          </div>
        ) : null}

        {isRouteReady && eligibleRouteOptions.length > 0 ? (
          <div className="mt-4 space-y-4">
            {capabilityRouteKindOrder.map((kind) => {
              const options = routeOptionsByKind.get(kind)
              if (!options || options.length === 0) {
                return null
              }

              return (
                <div className="space-y-2" key={kind}>
                  <h3 className="font-medium text-sm">{t(capabilityRouteKindLabelKeys[kind])}</h3>
                  <div className="space-y-2">
                    {options.map((option) => {
                      const savedRoute = findSavedCapabilityRoute(savedCapabilityRoutes, option)
                      const profileLabel =
                        profiles.find((profile) => profile.id === option.configId)?.displayName ??
                        option.configId
                      const routeStatus = savedRoute
                        ? savedRoute.enabled
                          ? t('provider.capabilityRouting.statusEnabled')
                          : t('provider.capabilityRouting.statusDisabled')
                        : t('provider.capabilityRouting.statusNotConfigured')
                      const isSavingRoute =
                        saveCapabilityRouteMutation.isPending &&
                        saveCapabilityRouteMutation.variables?.route.kind === option.kind &&
                        saveCapabilityRouteMutation.variables.route.configId === option.configId &&
                        saveCapabilityRouteMutation.variables.route.providerId === option.providerId
                      const isDeletingRoute =
                        deleteCapabilityRouteMutation.isPending &&
                        deleteCapabilityRouteMutation.variables?.kind === option.kind &&
                        deleteCapabilityRouteMutation.variables.configId === option.configId &&
                        deleteCapabilityRouteMutation.variables.providerId === option.providerId

                      return (
                        <div
                          className="rounded-md border border-border bg-surface px-3 py-3"
                          key={`${option.kind}:${option.configId}:${option.operationId}`}
                        >
                          <div className="flex flex-wrap items-start justify-between gap-3">
                            <div className="min-w-0 space-y-2">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="font-medium text-sm">{profileLabel}</span>
                                <Badge variant={savedRoute?.enabled ? 'success' : 'outline'}>
                                  {routeStatus}
                                </Badge>
                              </div>
                              <dl className="grid gap-2 text-sm md:grid-cols-2">
                                <div>
                                  <dt className="text-muted-foreground text-xs">
                                    {t('provider.capabilityRouting.operationId')}
                                  </dt>
                                  <dd className="mt-1 break-all font-mono text-xs">
                                    {option.operationId}
                                  </dd>
                                </div>
                                <div>
                                  <dt className="text-muted-foreground text-xs">
                                    {t('provider.capabilityRouting.outputArtifact')}
                                  </dt>
                                  <dd className="mt-1">
                                    <Badge variant="outline">
                                      {t(
                                        `provider.capabilityRouting.outputArtifactKind.${option.outputArtifact}`,
                                      )}
                                    </Badge>
                                  </dd>
                                </div>
                                <div>
                                  <dt className="text-muted-foreground text-xs">
                                    {t('provider.capabilityRouting.execution')}
                                  </dt>
                                  <dd className="mt-1">
                                    <Badge variant="outline">
                                      {t(
                                        `provider.capabilityRouting.executionMode.${option.execution}`,
                                      )}
                                    </Badge>
                                  </dd>
                                </div>
                                <div>
                                  <dt className="text-muted-foreground text-xs">
                                    {t('provider.capabilityRouting.costRisk')}
                                  </dt>
                                  <dd className="mt-1">
                                    <Badge variant="outline">
                                      {t(
                                        `provider.capabilityRouting.costRiskLevel.${option.costRisk}`,
                                      )}
                                    </Badge>
                                  </dd>
                                </div>
                              </dl>
                            </div>
                            <div className="flex shrink-0 flex-wrap gap-2">
                              {savedRoute?.enabled ? (
                                <Button
                                  disabled={isLoading || isRouteMutationPending}
                                  onClick={() =>
                                    void deleteCapabilityRouteMutation.mutateAsync({
                                      kind: option.kind,
                                      configId: option.configId,
                                      providerId: option.providerId,
                                    })
                                  }
                                  size="sm"
                                  type="button"
                                  variant="outline"
                                >
                                  {isDeletingRoute
                                    ? t('provider.capabilityRouting.disablingRoute')
                                    : t('provider.capabilityRouting.disableRoute')}
                                </Button>
                              ) : (
                                <Button
                                  disabled={isLoading || isRouteMutationPending}
                                  onClick={() =>
                                    void saveCapabilityRouteMutation.mutateAsync({
                                      route: buildCapabilityRouteFromOption(
                                        eligibleRouteOptions,
                                        option,
                                        true,
                                      ),
                                    })
                                  }
                                  size="sm"
                                  type="button"
                                >
                                  {isSavingRoute
                                    ? t('provider.capabilityRouting.enablingRoute')
                                    : t('provider.capabilityRouting.enableRoute')}
                                </Button>
                              )}
                            </div>
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </div>
              )
            })}
          </div>
        ) : null}
      </section>
    </div>
  )
}

function optionalTrimmed(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

function findSavedCapabilityRoute(
  routes: ProviderCapabilityRoute[],
  option: ProviderCapabilityRouteOption,
) {
  return routes.find(
    (route) =>
      route.kind === option.kind &&
      route.configId === option.configId &&
      route.providerId === option.providerId,
  )
}

function buildCapabilityRouteFromOption(
  eligibleOptions: ProviderCapabilityRouteOption[],
  option: ProviderCapabilityRouteOption,
  enabled: boolean,
): SaveProviderCapabilityRouteRequest['route'] {
  const operationIds = [
    ...new Set(
      eligibleOptions
        .filter(
          (candidate) =>
            candidate.kind === option.kind &&
            candidate.configId === option.configId &&
            candidate.providerId === option.providerId,
        )
        .map((candidate) => candidate.operationId),
    ),
  ].sort()

  return {
    kind: option.kind,
    configId: option.configId,
    providerId: option.providerId,
    operationIds,
    enabled,
  }
}

function setFormFromProvider(
  provider: ProviderCatalogEntry,
  setValue: UseFormSetValue<ProviderSettingsFormValues>,
) {
  setValue('apiKey', '')
  setValue('baseUrl', defaultBaseUrlForProvider(provider))
  setValue('configId', '')
  setValue('displayName', provider.displayName)
  setValue('modelId', runnableModels(provider)[0]?.modelId ?? '')
  setValue('providerId', provider.providerId)
}

function createFormValuesFromProvider(provider: ProviderCatalogEntry): ProviderSettingsFormValues {
  return {
    apiKey: '',
    baseUrl: defaultBaseUrlForProvider(provider),
    configId: '',
    displayName: provider.displayName,
    modelId: runnableModels(provider)[0]?.modelId ?? '',
    providerId: provider.providerId,
  }
}

function createFormValuesFromProfile(profile: ProviderConfig): ProviderSettingsFormValues {
  return {
    apiKey: '',
    baseUrl: profile.baseUrl ?? '',
    configId: profile.id,
    displayName: profile.displayName,
    modelId: profile.modelId,
    providerId: profile.providerId,
  }
}

function defaultBaseUrlForProvider(provider: ProviderCatalogEntry): string {
  if (provider.providerId === 'minimax') {
    return MINIMAX_BASE_URLS.international
  }
  return provider.defaultBaseUrl
}

function runnableModels(
  provider: ProviderCatalogEntry | undefined,
  profiles: ProviderConfig[] = [],
): ModelCatalogEntry[] {
  const models = new Map<string, ModelCatalogEntry>()
  for (const model of provider?.models ?? []) {
    if (model.runtimeStatus.kind === 'runnable') {
      models.set(model.modelId, model)
    }
  }
  for (const profile of profiles) {
    if (
      profile.providerId === provider?.providerId &&
      profile.modelDescriptor.runtimeStatus.kind === 'runnable'
    ) {
      models.set(profile.modelDescriptor.modelId, profile.modelDescriptor)
    }
  }
  return [...models.values()]
}
