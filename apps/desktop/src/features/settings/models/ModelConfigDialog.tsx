import { useEffect, useMemo, useRef } from 'react'
import { type UseFormSetValue, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

import {
  type ModelProviderCatalogResponse,
  type ProviderConfig,
  type ProviderSettingsRequest,
  saveProviderSettings,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Select } from '@/shared/ui/select'

type ModelConfigDialogProps = {
  catalog: ModelProviderCatalogResponse
  open: boolean
  profile?: ProviderConfig | null
  onOpenChange: (open: boolean) => void
  onSaved?: (config: ProviderConfig) => void
}

type ModelProtocol = NonNullable<ProviderSettingsRequest['protocol']>

type ModelConfigFormValues = {
  advancedBodyJson: string
  baseUrl: string
  codeInterpreter: boolean
  displayName: string
  enableThinking: boolean
  modelId: string
  outputEffort: string
  performanceLatency: string
  protocol: ModelProtocol
  providerId: string
  reasoningEffort: string
  responseMimeType: string
  seed: string
  serviceTier: string
  sessionCache: boolean
  stopSequences: string
  thinkingBudget: string
  thinkingType: string
  topK: string
  topP: string
  webExtractor: boolean
  webSearch: boolean
}

export function ModelConfigDialog({
  catalog,
  onOpenChange,
  onSaved,
  open,
  profile,
}: ModelConfigDialogProps) {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const formRef = useRef<HTMLFormElement>(null)
  const providers = catalog.providers
  const defaultProvider = providers[0]
  const defaultModel = defaultProvider?.models[0]
  const {
    formState: { errors, isSubmitting },
    handleSubmit,
    register,
    reset,
    setError,
    setValue,
    watch,
  } = useForm<ModelConfigFormValues>({
    defaultValues: formValuesFromProfile(profile, defaultProvider, defaultModel),
  })
  const providerId = watch('providerId')
  const modelId = watch('modelId')
  const selectedProvider = useMemo(
    () => providers.find((provider) => provider.providerId === providerId) ?? defaultProvider,
    [defaultProvider, providerId, providers],
  )
  const isQwen = selectedProvider?.providerId === 'qwen'
  const isDoubao = selectedProvider?.providerId === 'doubao'
  const protocol = watch('protocol')
  const qwenChatWebExtractorEnabled =
    protocol !== 'chat_completions' || supportsQwenChatWebExtractor(modelId)
  const modelOptions = useMemo(() => {
    const models = [...(selectedProvider?.models ?? [])]
    if (
      profile &&
      profile.providerId === selectedProvider?.providerId &&
      !models.some((model) => model.modelId === profile.modelId)
    ) {
      models.push({
        ...profile.modelDescriptor,
        displayName: profile.modelId,
        modelId: profile.modelId,
      })
    }
    return models
  }, [profile, selectedProvider])
  const selectedModel = useMemo(
    () => modelOptions.find((model) => model.modelId === modelId) ?? modelOptions[0],
    [modelId, modelOptions],
  )
  const supportedParameters = useMemo(
    () => new Set(selectedModel?.supportedParameters ?? []),
    [selectedModel],
  )

  useEffect(() => {
    if (open) {
      reset(formValuesFromProfile(profile, defaultProvider, defaultModel))
    }
  }, [defaultModel, defaultProvider, open, profile, reset])

  useEffect(() => {
    if (!open || !selectedProvider) {
      return
    }

    const modelExists = modelOptions.some((model) => model.modelId === modelId)
    const firstModel = modelOptions[0]
    if (!modelExists && firstModel) {
      setValue('modelId', firstModel.modelId)
    }
  }, [modelId, modelOptions, open, selectedProvider, setValue])

  function changeOpen(nextOpen: boolean) {
    if (!nextOpen) {
      clearSecretFormFields(formRef.current)
      reset(formValuesFromProfile(profile, defaultProvider, defaultModel))
    }
    onOpenChange(nextOpen)
  }

  async function submit(values: ModelConfigFormValues, form: HTMLFormElement) {
    const request: ProviderSettingsRequest = {
      modelId: values.modelId,
      providerId: values.providerId,
    }
    const displayName = values.displayName.trim()
    const baseUrl = values.baseUrl.trim()
    const apiKey = readSecretFormValue(form, 'apiKey')
    const officialQuotaApiKey = readSecretFormValue(form, 'officialQuotaApiKey')

    if (profile) {
      request.configId = profile.id
      request.setDefault = profile.isDefault
    }
    request.modelOptions = {}
    if (displayName) {
      request.displayName = displayName
    }
    if (baseUrl) {
      request.baseUrl = baseUrl
    }
    try {
      const providerDefaults = providerDefaultsFromValues(values, supportedParameters)
      if (values.providerId === 'qwen') {
        request.protocol = values.protocol
        request.providerDefaults = providerDefaults
      } else if (hasProviderDefaults(providerDefaults)) {
        request.providerDefaults = providerDefaults
      }
    } catch (error) {
      setError('root', { message: getCommandErrorMessage(error) })
      return
    }
    if (apiKey) {
      request.apiKey = apiKey
    }
    if (officialQuotaApiKey) {
      request.officialQuotaApiKey = officialQuotaApiKey
    }
    if (!profile?.hasApiKey && !apiKey) {
      setError('root', { message: t('provider.errors.apiKeyRequired') })
      return
    }

    try {
      const response = await saveProviderSettings(request, commandClient)
      clearSecretFormFields(form)
      reset(values)
      onSaved?.(response.config)
      changeOpen(false)
    } catch (error) {
      clearSecretFormFields(form)
      setError('root', { message: getCommandErrorMessage(error) })
    }
  }

  return (
    <Dialog onOpenChange={changeOpen} open={open}>
      <DialogContent className="w-[min(calc(100vw-2rem),36rem)]">
        <DialogHeader>
          <DialogTitle>
            {profile ? t('models.configDialog.editTitle') : t('provider.createTitle')}
          </DialogTitle>
          <DialogDescription>
            {profile ? t('models.configDialog.editDescription') : t('provider.createDescription')}
          </DialogDescription>
        </DialogHeader>

        <form
          className="grid gap-4"
          ref={formRef}
          onSubmit={(event) => {
            const form = event.currentTarget
            void handleSubmit((values) => submit(values, form))(event)
          }}
        >
          <label className="grid gap-1 text-sm" htmlFor="provider-display-name">
            <span className="font-medium">{t('provider.profileName')}</span>
            <Input id="provider-display-name" {...register('displayName')} />
          </label>

          <label className="grid gap-1 text-sm" htmlFor="provider-provider-id">
            <span className="font-medium">{t('provider.provider')}</span>
            <Select
              id="provider-provider-id"
              {...register('providerId', {
                required: t('provider.errors.providerRequired'),
                onChange: (event) => {
                  const provider = providers.find(
                    (candidate) => candidate.providerId === event.target.value,
                  )
                  setValue('baseUrl', provider?.defaultBaseUrl ?? '')
                  setValue('modelId', provider?.models[0]?.modelId ?? '')
                  setValue('protocol', defaultProtocolForProvider(provider))
                  resetProviderOptionFields(setValue)
                },
              })}
            >
              {providers.map((provider) => (
                <option key={provider.providerId} value={provider.providerId}>
                  {provider.displayName}
                </option>
              ))}
            </Select>
          </label>

          <label className="grid gap-1 text-sm" htmlFor="provider-model-id">
            <span className="font-medium">{t('provider.model')}</span>
            <Select
              id="provider-model-id"
              {...register('modelId', {
                required: t('provider.errors.modelRequired'),
              })}
            >
              {modelOptions.map((model) => (
                <option key={model.modelId} value={model.modelId}>
                  {model.displayName}
                </option>
              ))}
            </Select>
          </label>

          {isQwen ? (
            <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
              <label className="grid gap-1" htmlFor="provider-protocol">
                <span className="font-medium">{t('provider.apiMode')}</span>
                <Select id="provider-protocol" {...register('protocol')}>
                  <option value="responses">Responses</option>
                  <option value="chat_completions">Chat Completions</option>
                </Select>
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" {...register('enableThinking')} />
                <span>{t('provider.enableThinking')}</span>
              </label>
              <label className="grid gap-1" htmlFor="provider-reasoning-effort">
                <span className="font-medium">{t('provider.reasoningEffort')}</span>
                <Select id="provider-reasoning-effort" {...register('reasoningEffort')}>
                  <option value="">{t('provider.default')}</option>
                  <option value="none">None</option>
                  <option value="minimal">Minimal</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                </Select>
              </label>
              <div className="grid gap-2">
                <span className="font-medium">{t('provider.builtinTools')}</span>
                <label className="flex items-center gap-2">
                  <input type="checkbox" {...register('webSearch')} />
                  <span>web_search</span>
                </label>
                <label className="flex items-center gap-2">
                  <input type="checkbox" {...register('codeInterpreter')} />
                  <span>code_interpreter</span>
                </label>
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    disabled={!qwenChatWebExtractorEnabled}
                    {...register('webExtractor')}
                  />
                  <span>web_extractor</span>
                </label>
              </div>
              <label className="flex items-center gap-2">
                <input type="checkbox" {...register('sessionCache')} />
                <span>{t('provider.sessionCache')}</span>
              </label>
            </div>
          ) : null}

          {!isQwen && supportedParameters.size > 0 ? (
            <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
              <span className="font-medium">{t('provider.providerOptions')}</span>
              {isDoubao && supportedParameters.has('thinking') ? (
                <label className="grid gap-1" htmlFor="provider-thinking-type">
                  <span className="font-medium">{t('provider.thinkingMode')}</span>
                  <Select id="provider-thinking-type" {...register('thinkingType')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="enabled">Enabled</option>
                    <option value="auto">Auto</option>
                    <option value="disabled">Disabled</option>
                  </Select>
                </label>
              ) : supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) ? (
                <label className="flex items-center gap-2">
                  <input type="checkbox" {...register('enableThinking')} />
                  <span>{t('provider.enableThinking')}</span>
                </label>
              ) : null}
              {supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) && !isDoubao ? (
                <label className="grid gap-1" htmlFor="provider-thinking-budget">
                  <span className="font-medium">{t('provider.thinkingBudget')}</span>
                  <Input
                    id="provider-thinking-budget"
                    inputMode="numeric"
                    {...register('thinkingBudget')}
                  />
                </label>
              ) : null}
              {supportedParameters.has('output_config') ? (
                <label className="grid gap-1" htmlFor="provider-output-effort">
                  <span className="font-medium">{t('provider.outputEffort')}</span>
                  <Select id="provider-output-effort" {...register('outputEffort')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="low">Low</option>
                    <option value="medium">Medium</option>
                    <option value="high">High</option>
                  </Select>
                </label>
              ) : null}
              {supportedParameters.has('reasoning_effort') ? (
                <label className="grid gap-1" htmlFor="provider-generic-reasoning-effort">
                  <span className="font-medium">{t('provider.reasoningEffort')}</span>
                  <Select id="provider-generic-reasoning-effort" {...register('reasoningEffort')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="none">None</option>
                    <option value="minimal">Minimal</option>
                    <option value="low">Low</option>
                    <option value="medium">Medium</option>
                    <option value="high">High</option>
                    <option value="xhigh">XHigh</option>
                    <option value="max">Max</option>
                  </Select>
                </label>
              ) : null}
              {supportedParameters.has('service_tier') ? (
                <label className="grid gap-1" htmlFor="provider-service-tier">
                  <span className="font-medium">{t('provider.serviceTier')}</span>
                  <Select id="provider-service-tier" {...register('serviceTier')}>
                    <option value="">{t('provider.default')}</option>
                    {isDoubao ? (
                      <>
                        <option value="fast">Fast</option>
                        <option value="auto">Auto</option>
                        <option value="default">Default</option>
                      </>
                    ) : (
                      <>
                        <option value="auto">Auto</option>
                        <option value="standard_only">Standard only</option>
                      </>
                    )}
                  </Select>
                </label>
              ) : null}
              {supportsAny(supportedParameters, ['top_p', 'topP']) ? (
                <label className="grid gap-1" htmlFor="provider-top-p">
                  <span className="font-medium">{t('provider.topP')}</span>
                  <Input id="provider-top-p" inputMode="decimal" {...register('topP')} />
                </label>
              ) : null}
              {supportsAny(supportedParameters, ['top_k', 'topK']) ? (
                <label className="grid gap-1" htmlFor="provider-top-k">
                  <span className="font-medium">{t('provider.topK')}</span>
                  <Input id="provider-top-k" inputMode="numeric" {...register('topK')} />
                </label>
              ) : null}
              {supportedParameters.has('seed') ? (
                <label className="grid gap-1" htmlFor="provider-seed">
                  <span className="font-medium">{t('provider.seed')}</span>
                  <Input id="provider-seed" inputMode="numeric" {...register('seed')} />
                </label>
              ) : null}
              {supportsAny(supportedParameters, ['stop_sequences', 'stopSequences']) ? (
                <label className="grid gap-1" htmlFor="provider-stop-sequences">
                  <span className="font-medium">{t('provider.stopSequences')}</span>
                  <Input id="provider-stop-sequences" {...register('stopSequences')} />
                </label>
              ) : null}
              {supportedParameters.has('responseMimeType') ? (
                <label className="grid gap-1" htmlFor="provider-response-mime-type">
                  <span className="font-medium">{t('provider.responseMimeType')}</span>
                  <Input id="provider-response-mime-type" {...register('responseMimeType')} />
                </label>
              ) : null}
              {supportedParameters.has('performanceConfig') ? (
                <label className="grid gap-1" htmlFor="provider-performance-latency">
                  <span className="font-medium">{t('provider.performanceLatency')}</span>
                  <Select id="provider-performance-latency" {...register('performanceLatency')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="standard">Standard</option>
                    <option value="optimized">Optimized</option>
                  </Select>
                </label>
              ) : null}
              <label className="grid gap-1" htmlFor="provider-advanced-body-json">
                <span className="font-medium">{t('provider.advancedBodyJson')}</span>
                <textarea
                  className="min-h-20 w-full rounded-md border border-input bg-background px-3 py-2 font-mono text-sm tracking-normal outline-none transition-[border-color,box-shadow] duration-200 placeholder:text-muted-foreground focus:border-ring/60 focus:ring-2 focus:ring-ring/10"
                  id="provider-advanced-body-json"
                  {...register('advancedBodyJson')}
                />
              </label>
            </div>
          ) : null}

          <label className="grid gap-1 text-sm" htmlFor="provider-base-url">
            <span className="font-medium">{t('provider.baseUrl')}</span>
            <Input
              id="provider-base-url"
              placeholder={selectedProvider?.defaultBaseUrl}
              {...register('baseUrl')}
            />
          </label>

          <label className="grid gap-1 text-sm" htmlFor="provider-api-key">
            <span className="font-medium">{t('provider.apiKey')}</span>
            <Input
              id="provider-api-key"
              placeholder={
                profile?.hasApiKey
                  ? t('provider.apiKeyExistingPlaceholder')
                  : t('provider.apiKeyPlaceholder')
              }
              type="password"
              name="apiKey"
            />
          </label>

          <label className="grid gap-1 text-sm" htmlFor="provider-official-quota-api-key">
            <span className="font-medium">{t('provider.officialQuotaApiKey')}</span>
            <Input
              id="provider-official-quota-api-key"
              placeholder={
                profile?.hasOfficialQuotaApiKey
                  ? t('provider.officialQuotaApiKeyExistingPlaceholder')
                  : t('provider.officialQuotaApiKeyPlaceholder')
              }
              type="password"
              name="officialQuotaApiKey"
            />
          </label>

          {errors.root?.message ? (
            <p className="text-destructive text-sm" role="alert">
              {errors.root.message}
            </p>
          ) : null}

          <DialogFooter>
            <Button
              disabled={isSubmitting}
              type="button"
              variant="outline"
              onClick={() => changeOpen(false)}
            >
              {t('models.configDialog.cancel')}
            </Button>
            <Button disabled={isSubmitting} type="submit">
              {isSubmitting ? t('provider.saving') : t('provider.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function formValuesFromProfile(
  profile: ProviderConfig | null | undefined,
  defaultProvider: ModelProviderCatalogResponse['providers'][number] | undefined,
  defaultModel: ModelProviderCatalogResponse['providers'][number]['models'][number] | undefined,
): ModelConfigFormValues {
  const defaults = qwenDefaultsFromProfile(profile)
  const providerDefaults = providerOptionDefaultsFromProfile(profile)
  const advancedDefaults = advancedProviderDefaultsFromProfile(profile)
  return {
    advancedBodyJson: advancedDefaults.body,
    baseUrl: profile?.baseUrl ?? defaultProvider?.defaultBaseUrl ?? '',
    codeInterpreter: defaults.codeInterpreter,
    displayName: profile?.displayName ?? '',
    enableThinking: defaults.enableThinking || providerDefaults.enableThinking,
    modelId: profile?.modelId ?? defaultModel?.modelId ?? '',
    outputEffort: providerDefaults.outputEffort,
    performanceLatency: providerDefaults.performanceLatency,
    protocol: profile?.protocol ?? defaultProtocolForProvider(defaultProvider),
    providerId: profile?.providerId ?? defaultProvider?.providerId ?? '',
    reasoningEffort: defaults.reasoningEffort || providerDefaults.reasoningEffort,
    responseMimeType: providerDefaults.responseMimeType,
    seed: providerDefaults.seed,
    serviceTier: providerDefaults.serviceTier,
    sessionCache: defaults.sessionCache,
    stopSequences: providerDefaults.stopSequences,
    thinkingBudget: providerDefaults.thinkingBudget,
    thinkingType: providerDefaults.thinkingType,
    topK: providerDefaults.topK,
    topP: providerDefaults.topP,
    webExtractor: defaults.webExtractor,
    webSearch: defaults.webSearch,
  }
}

function defaultProtocolForProvider(
  provider: ModelProviderCatalogResponse['providers'][number] | undefined,
): ModelProtocol {
  return (
    (provider?.providerId === 'qwen' ? 'responses' : provider?.models[0]?.protocol) ?? 'responses'
  )
}

function qwenDefaultsFromProfile(profile: ProviderConfig | null | undefined) {
  const body = profile?.providerDefaults?.body ?? {}
  const tools = Array.isArray(body.tools) ? body.tools : []
  const toolTypes = new Set(
    tools
      .map((tool) => (isRecord(tool) && typeof tool.type === 'string' ? tool.type : null))
      .filter((tool): tool is string => tool !== null),
  )
  const reasoning = isRecord(body.reasoning) ? body.reasoning : null
  const reasoningEffort = typeof reasoning?.effort === 'string' ? reasoning.effort : ''
  const searchOptions = isRecord(body.search_options) ? body.search_options : null
  const qwenChatWebExtractor = searchOptions?.search_strategy === 'agent_max'
  const headers = profile?.providerDefaults?.headers ?? {}
  const sessionCache = Object.entries(headers).some(
    ([name, value]) => name.toLowerCase() === 'x-dashscope-session-cache' && value === 'enable',
  )

  return {
    codeInterpreter: toolTypes.has('code_interpreter') || body.enable_code_interpreter === true,
    enableThinking: body.enable_thinking === true,
    reasoningEffort,
    sessionCache,
    webExtractor: toolTypes.has('web_extractor') || qwenChatWebExtractor,
    webSearch: toolTypes.has('web_search') || body.enable_search === true,
  }
}

function providerOptionDefaultsFromProfile(profile: ProviderConfig | null | undefined) {
  const body = profile?.providerDefaults?.body ?? {}
  const thinking = isRecord(body.thinking) ? body.thinking : null
  const thinkingConfig = isRecord(body.thinkingConfig) ? body.thinkingConfig : null
  const outputConfig = isRecord(body.output_config) ? body.output_config : null
  const inferenceConfig = isRecord(body.inferenceConfig) ? body.inferenceConfig : null
  const performanceConfig = isRecord(body.performanceConfig) ? body.performanceConfig : null
  const topP = firstStringable(body.top_p, body.topP, inferenceConfig?.topP)
  const topK = firstStringable(body.top_k, body.topK)
  const stopSequences = firstArray(
    body.stop_sequences,
    body.stopSequences,
    inferenceConfig?.stopSequences,
  )

  return {
    enableThinking:
      thinking !== null ||
      thinkingConfig !== null ||
      body.enable_thinking === true ||
      body.enableThinking === true,
    outputEffort: typeof outputConfig?.effort === 'string' ? outputConfig.effort : '',
    performanceLatency:
      typeof performanceConfig?.latency === 'string' ? performanceConfig.latency : '',
    responseMimeType: typeof body.responseMimeType === 'string' ? body.responseMimeType : '',
    reasoningEffort: typeof body.reasoning_effort === 'string' ? body.reasoning_effort : '',
    seed: firstStringable(body.seed),
    serviceTier: typeof body.service_tier === 'string' ? body.service_tier : '',
    stopSequences: stopSequences.join(','),
    thinkingBudget: firstStringable(thinking?.budget_tokens, thinkingConfig?.thinkingBudget),
    thinkingType: typeof thinking?.type === 'string' ? thinking.type : '',
    topK,
    topP,
  }
}

function providerDefaultsFromValues(
  values: ModelConfigFormValues,
  supportedParameters: Set<string>,
): ProviderSettingsRequest['providerDefaults'] {
  const body: Record<string, unknown> = {}
  const headers: Record<string, string> = {}
  const tools: Array<{ type: string }> = []

  if (values.providerId !== 'qwen') {
    const stopSequences = parseList(values.stopSequences)
    const topP = parseNumber(values.topP)
    const topK = parseNumber(values.topK)
    const seed = parseNumber(values.seed)
    const thinkingBudget = parseNumber(values.thinkingBudget)

    if (values.providerId === 'anthropic') {
      if (values.enableThinking) {
        body.thinking =
          thinkingBudget !== null
            ? { type: 'enabled', budget_tokens: thinkingBudget }
            : { type: 'enabled' }
      }
      if (values.outputEffort) {
        body.output_config = { effort: values.outputEffort }
      }
      if (values.serviceTier) {
        body.service_tier = values.serviceTier
      }
      if (stopSequences.length > 0) {
        body.stop_sequences = stopSequences
      }
      if (topP !== null) {
        body.top_p = topP
      }
      if (topK !== null) {
        body.top_k = topK
      }
      return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
    }

    if (values.providerId === 'gemini') {
      if (values.enableThinking || thinkingBudget !== null) {
        body.thinkingConfig =
          thinkingBudget !== null ? { thinkingBudget } : { includeThoughts: true }
      }
      if (stopSequences.length > 0) {
        body.stopSequences = stopSequences
      }
      if (topP !== null) {
        body.topP = topP
      }
      if (topK !== null) {
        body.topK = topK
      }
      if (seed !== null) {
        body.seed = seed
      }
      if (values.responseMimeType.trim()) {
        body.responseMimeType = values.responseMimeType.trim()
      }
      return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
    }

    if (values.providerId === 'bedrock') {
      const inferenceConfig: Record<string, unknown> = {}
      if (topP !== null) {
        inferenceConfig.topP = topP
      }
      if (stopSequences.length > 0) {
        inferenceConfig.stopSequences = stopSequences
      }
      if (Object.keys(inferenceConfig).length > 0) {
        body.inferenceConfig = inferenceConfig
      }
      if (values.performanceLatency) {
        body.performanceConfig = { latency: values.performanceLatency }
      }
      return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
    }

    if (topP !== null) {
      body.top_p = topP
    }
    if (topK !== null) {
      body.top_k = topK
    }
    if (stopSequences.length > 0) {
      body.stop = stopSequences
    }
    if (values.serviceTier) {
      body.service_tier = values.serviceTier
    }
    if (values.providerId === 'doubao' && values.thinkingType) {
      body.thinking = { type: values.thinkingType }
    } else if (values.enableThinking) {
      body.thinking = { type: 'enabled' }
    }
    if (values.reasoningEffort) {
      body.reasoning_effort = values.reasoningEffort
    }
    return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
  }

  if (values.enableThinking) {
    body.enable_thinking = true
  }
  if (values.reasoningEffort) {
    body.reasoning = { effort: values.reasoningEffort }
  }
  if (values.protocol === 'responses') {
    if (values.webSearch) {
      tools.push({ type: 'web_search' })
    }
    if (values.codeInterpreter) {
      tools.push({ type: 'code_interpreter' })
    }
    if (values.webExtractor) {
      tools.push({ type: 'web_extractor' })
    }
    if (tools.length > 0) {
      body.tools = tools
    }
  } else {
    const chatWebExtractor = values.webExtractor && supportsQwenChatWebExtractor(values.modelId)
    if (values.webSearch || chatWebExtractor) {
      body.enable_search = true
    }
    if (values.codeInterpreter) {
      body.enable_code_interpreter = true
    }
    if (chatWebExtractor) {
      body.enable_thinking = true
      body.search_options = { search_strategy: 'agent_max' }
    }
  }
  if (values.sessionCache) {
    headers['x-dashscope-session-cache'] = 'enable'
  }

  return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
}

function hasProviderDefaults(defaults: ProviderSettingsRequest['providerDefaults']): boolean {
  return (
    Object.keys(defaults?.body ?? {}).length > 0 || Object.keys(defaults?.headers ?? {}).length > 0
  )
}

function resetProviderOptionFields(setValue: UseFormSetValue<ModelConfigFormValues>) {
  setValue('advancedBodyJson', '')
  setValue('enableThinking', false)
  setValue('outputEffort', '')
  setValue('performanceLatency', '')
  setValue('reasoningEffort', '')
  setValue('responseMimeType', '')
  setValue('seed', '')
  setValue('serviceTier', '')
  setValue('stopSequences', '')
  setValue('thinkingBudget', '')
  setValue('thinkingType', '')
  setValue('topK', '')
  setValue('topP', '')
  setValue('webSearch', false)
  setValue('codeInterpreter', false)
  setValue('webExtractor', false)
  setValue('sessionCache', false)
}

function mergeAdvancedProviderDefaults(
  body: Record<string, unknown>,
  headers: Record<string, string>,
  values: ModelConfigFormValues,
  supportedParameters: Set<string>,
): ProviderSettingsRequest['providerDefaults'] {
  Object.assign(body, parseJsonRecord(values.advancedBodyJson))
  pruneUnsupportedManagedProviderDefaults(body, values.providerId, supportedParameters)
  return { body, headers }
}

const MANAGED_PROVIDER_BODY_KEYS = new Set([
  'enable_code_interpreter',
  'enable_search',
  'enable_thinking',
  'enableThinking',
  'inferenceConfig',
  'output_config',
  'performanceConfig',
  'reasoning',
  'reasoning_effort',
  'responseMimeType',
  'search_options',
  'seed',
  'service_tier',
  'stop',
  'stop_sequences',
  'stopSequences',
  'thinking',
  'thinkingConfig',
  'tools',
  'top_k',
  'top_p',
  'topK',
  'topP',
])

function pruneUnsupportedManagedProviderDefaults(
  body: Record<string, unknown>,
  providerId: string,
  supportedParameters: Set<string>,
) {
  if (providerId === 'qwen') {
    return
  }
  for (const key of Object.keys(body)) {
    if (
      MANAGED_PROVIDER_BODY_KEYS.has(key) &&
      !providerDefaultBodyKeySupported(key, supportedParameters)
    ) {
      delete body[key]
    }
  }
}

function providerDefaultBodyKeySupported(key: string, supportedParameters: Set<string>): boolean {
  switch (key) {
    case 'thinking':
      return supportsAny(supportedParameters, ['thinking'])
    case 'thinkingConfig':
      return supportsAny(supportedParameters, ['thinkingConfig'])
    default:
      return supportedParameters.has(key)
  }
}

function advancedProviderDefaultsFromProfile(profile: ProviderConfig | null | undefined): {
  body: string
} {
  const body = pickUnmanagedBodyDefaults(profile?.providerDefaults?.body ?? {})
  return {
    body: stringifyJsonRecord(body),
  }
}

function pickUnmanagedBodyDefaults(body: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(body).filter(([name]) => !MANAGED_PROVIDER_BODY_KEYS.has(name)),
  )
}

function parseJsonRecord(value: string): Record<string, unknown> {
  const trimmed = value.trim()
  if (!trimmed) {
    return {}
  }
  const parsed: unknown = JSON.parse(trimmed)
  return isRecord(parsed) ? parsed : {}
}

function stringifyJsonRecord(value: Record<string, unknown>): string {
  return Object.keys(value).length > 0 ? JSON.stringify(value, null, 2) : ''
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function firstStringable(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value === 'string') {
      return value
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
      return String(value)
    }
  }
  return ''
}

function firstArray(...values: unknown[]): string[] {
  for (const value of values) {
    if (Array.isArray(value)) {
      return value.filter((item): item is string => typeof item === 'string')
    }
  }
  return []
}

function parseNumber(value: string): number | null {
  if (!value.trim()) {
    return null
  }
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

function parseList(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

function supportsAny(supportedParameters: Set<string>, parameters: string[]): boolean {
  return parameters.some((parameter) => supportedParameters.has(parameter))
}

function supportsQwenChatWebExtractor(modelId: string): boolean {
  return modelId === 'qwen3-max' || modelId === 'qwen3-max-2026-01-23'
}

function readSecretFormValue(form: HTMLFormElement, name: string): string {
  const value = new FormData(form).get(name)
  return typeof value === 'string' ? value.trim() : ''
}

function clearSecretFormFields(form: HTMLFormElement | null) {
  if (!form) {
    return
  }
  for (const name of ['apiKey', 'officialQuotaApiKey']) {
    const field = form.elements.namedItem(name)
    if (field instanceof HTMLInputElement) {
      field.value = ''
    }
  }
}
