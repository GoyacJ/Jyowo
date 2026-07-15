import { ChevronRight } from 'lucide-react'
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
import { Textarea } from '@/shared/ui/textarea'

type ModelConfigDialogProps = {
  catalog: ModelProviderCatalogResponse
  open: boolean
  profile?: ProviderConfig | null
  onOpenChange: (open: boolean) => void
  onSaved?: (config: ProviderConfig) => void
}

type ModelProtocol = NonNullable<ProviderSettingsRequest['protocol']>
type OpenAiResponsesOptions = NonNullable<
  NonNullable<ProviderSettingsRequest['modelOptions']>['openaiResponses']
>

type ModelConfigFormValues = {
  advancedBodyJson: string
  baseUrl: string
  clearThinking: string
  codeInterpreter: boolean
  displayName: string
  doSample: string
  enableThinking: boolean
  thinkingType: string
  thinkingDisplay: string
  cacheTtl: string
  maxTokens: string
  kimiPartialContent: string
  kimiPartialName: string
  modelId: string
  outputEffort: string
  performanceLatency: string
  promptCacheKey: string
  preserveThinking: boolean
  protocol: ModelProtocol
  providerId: string
  reasoningEffort: string
  responseFormat: string
  cachedContent: string
  responseMimeType: string
  responseJsonSchema: string
  seed: string
  serviceTier: string
  safetyIdentifier: string
  sessionCache: boolean
  stopSequences: string
  storeResponse: boolean
  thinkingBudget: string
  thinkingMode: string
  temperature: string
  toolStream: string
  thinkingLevel: string
  toolConfig: string
  safetySettings: string
  topK: string
  topP: string
  toolChoice: string
  toolName: string
  metadataJson: string
  anthropicBeta: string
  anthropicUserProfileId: string
  inferenceGeo: string
  speed: string
  fallbacksJson: string
  contextManagementJson: string
  anthropicAdvancedJson: string
  userId: string
  openaiAdvancedJson: string
  openaiBackground: boolean
  openaiConversationJson: string
  openaiInclude: string
  openaiInstructions: string
  openaiMaxToolCalls: string
  openaiMetadataJson: string
  openaiParallelToolCalls: boolean
  openaiPromptCacheKey: string
  openaiPromptCacheRetention: string
  openaiPromptJson: string
  openaiReasoningContext: string
  openaiReasoningEffort: string
  openaiReasoningSummary: string
  openaiSafetyIdentifier: string
  openaiServiceTier: string
  openaiStore: boolean
  openaiStrictToolSchemas: boolean
  openaiTextFormatJson: string
  openaiTextVerbosity: string
  openaiToolChoiceJson: string
  openaiTopLogprobs: string
  openaiTopP: string
  openaiTruncation: string
  openaiUser: string
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
  const defaultModel = firstRunnableModel(defaultProvider)
  const {
    formState: { errors, isSubmitting },
    clearErrors,
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
  const isOpenAI = selectedProvider?.providerId === 'openai'
  const isAnthropic = selectedProvider?.providerId === 'anthropic'
  const isDeepSeek = selectedProvider?.providerId === 'deepseek'
  const isZhipu = selectedProvider?.providerId === 'zhipu'
  const isDoubao = selectedProvider?.providerId === 'doubao'
  const protocol = watch('protocol')
  const thinkingMode = watch('thinkingMode')
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
    () => modelOptions.find((model) => model.modelId === modelId),
    [modelId, modelOptions],
  )
  const selectedModelIsRunnable = selectedModel?.runtimeStatus.kind === 'runnable'
  const providerCapabilityMetadata = getAnthropicCapabilityMetadata(
    selectedModel?.providerCapabilityMetadata,
  )
  const anthropicSamplingLocked = providerCapabilityMetadata?.samplingLocked === true
  const protocolOptions = selectedModel?.supportedProtocols?.length
    ? selectedModel.supportedProtocols
    : selectedModel
      ? [selectedModel.protocol]
      : []
  const serviceTierOptions = isDoubao
    ? ['fast', 'auto', 'default']
    : (providerCapabilityMetadata?.serviceTiers ?? ['auto', 'standard_only'])
  const supportedParameters = useMemo(
    () =>
      new Set(
        providerCapabilityMetadata?.protocolSupportedParameters?.[protocol] ??
          selectedModel?.supportedParameters ??
          [],
      ),
    [protocol, providerCapabilityMetadata, selectedModel],
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

    const model = modelOptions.find((candidate) => candidate.modelId === modelId)
    const isExistingProfileModel =
      profile?.providerId === selectedProvider.providerId && profile.modelId === modelId
    if (model?.runtimeStatus.kind !== 'runnable' && !isExistingProfileModel) {
      setValue('modelId', firstRunnableModel(selectedProvider)?.modelId ?? '')
    }
  }, [modelId, modelOptions, open, profile, selectedProvider, setValue])

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
    const invalidJsonField = invalidGeminiJsonField(values)
    if (invalidJsonField) {
      setError('root', {
        message: t('provider.errors.invalidJsonField', {
          field: invalidJsonField,
        }),
      })
      return
    }
    clearErrors('root')

    if (profile) {
      request.configId = profile.id
      request.setDefault = profile.isDefault
    }
    if (profile) {
      request.modelOptions = {}
    }
    if (values.providerId === 'openai') {
      try {
        const openaiResponses = openAiResponsesOptionsFromValues(values)
        if (hasOpenAiResponsesOptions(openaiResponses)) {
          request.modelOptions = { openaiResponses }
        }
      } catch (error) {
        setError('root', {
          message: error instanceof Error ? error.message : 'Invalid OpenAI options',
        })
        return
      }
    }
    if (values.providerId === 'km') {
      const modelOptions = modelOptionsFromValues(values)
      if (profile || hasModelOptions(modelOptions)) {
        request.modelOptions = modelOptions
      }
    }
    if (displayName) {
      request.displayName = displayName
    }
    if (baseUrl) {
      request.baseUrl = baseUrl
    }
    try {
      const providerDefaults = providerDefaultsFromValues(values, supportedParameters)
      if (providerPersistsProtocol(values.providerId)) {
        request.protocol = values.protocol
      }
      if (values.providerId === 'openai') {
        // OpenAI Responses request fields are typed model options, not provider defaults.
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
    const apiKeyRequired = selectedProvider?.runtimeCapability.authScheme !== 'none'
    if (apiKeyRequired && !profile?.hasApiKey && !apiKey) {
      setError('root.apiKey', { message: t('provider.errors.apiKeyRequired') })
      const apiKeyField = form.elements.namedItem('apiKey')
      if (apiKeyField instanceof HTMLInputElement) {
        apiKeyField.focus()
      }
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
      <DialogContent className="max-h-[calc(100vh-2rem)] w-[min(calc(100vw-2rem),36rem)] grid-rows-[auto_minmax(0,1fr)] gap-0 overflow-hidden p-0">
        <DialogHeader className="border-border border-b px-6 py-5 pr-12">
          <DialogTitle>
            {profile ? t('models.configDialog.editTitle') : t('provider.createTitle')}
          </DialogTitle>
          <DialogDescription>
            {profile ? t('models.configDialog.editDescription') : t('provider.createDescription')}
          </DialogDescription>
        </DialogHeader>

        <form
          className="flex min-h-0 flex-col overflow-hidden"
          ref={formRef}
          onSubmit={(event) => {
            const form = event.currentTarget
            void handleSubmit((values) => submit(values, form))(event)
          }}
        >
          <div className="grid min-h-0 gap-4 overflow-y-auto px-6 py-5">
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
                    const model = firstRunnableModel(provider)
                    setValue('baseUrl', provider?.defaultBaseUrl ?? '')
                    setValue('modelId', model?.modelId ?? '')
                    setValue('protocol', defaultProtocolForModel(model))
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
                  validate: (value) =>
                    modelOptions.some(
                      (model) => model.modelId === value && model.runtimeStatus.kind === 'runnable',
                    ) || t('provider.errors.modelRequired'),
                  onChange: (event) => {
                    const model = modelOptions.find(
                      (candidate) => candidate.modelId === event.target.value,
                    )
                    setValue('protocol', defaultProtocolForModel(model))
                  },
                })}
              >
                {!selectedModel ? <option hidden value="" /> : null}
                {modelOptions.map((model) => (
                  <option
                    disabled={model.runtimeStatus.kind !== 'runnable'}
                    key={model.modelId}
                    value={model.modelId}
                  >
                    {model.displayName}
                  </option>
                ))}
              </Select>
            </label>

            <label className="grid gap-1 text-sm" htmlFor="provider-api-key">
              <span className="font-medium">{t('provider.apiKey')}</span>
              <Input
                aria-describedby={
                  errors.root?.apiKey?.message ? 'provider-api-key-error' : undefined
                }
                aria-invalid={errors.root?.apiKey?.message ? true : undefined}
                aria-required={
                  selectedProvider?.runtimeCapability.authScheme !== 'none' && !profile?.hasApiKey
                }
                id="provider-api-key"
                placeholder={
                  profile?.hasApiKey
                    ? t('provider.apiKeyExistingPlaceholder')
                    : t('provider.apiKeyPlaceholder')
                }
                type="password"
                name="apiKey"
              />
              {errors.root?.apiKey?.message ? (
                <span className="text-destructive text-sm" id="provider-api-key-error" role="alert">
                  {errors.root.apiKey.message}
                </span>
              ) : null}
            </label>

            <details className="group rounded-md border border-border">
              <summary className="flex cursor-pointer list-none items-start gap-3 rounded-md px-4 py-3 outline-none focus-visible:ring-2 focus-visible:ring-ring">
                <ChevronRight
                  aria-hidden="true"
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
                />
                <span className="grid gap-0.5 text-sm">
                  <span className="font-medium">{t('provider.connectionOptions')}</span>
                  <span className="text-muted-foreground">
                    {t('provider.connectionOptionsDescription')}
                  </span>
                </span>
              </summary>
              <div className="grid gap-4 border-border border-t px-4 py-4">
                <label className="grid gap-1 text-sm" htmlFor="provider-display-name">
                  <span className="font-medium">{t('provider.profileName')}</span>
                  <Input id="provider-display-name" {...register('displayName')} />
                </label>

                <label className="grid gap-1 text-sm" htmlFor="provider-base-url">
                  <span className="font-medium">{t('provider.baseUrl')}</span>
                  <Input
                    id="provider-base-url"
                    placeholder={selectedProvider?.defaultBaseUrl}
                    {...register('baseUrl')}
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
              </div>
            </details>

            <details className="group rounded-md border border-border">
              <summary className="flex cursor-pointer list-none items-start gap-3 rounded-md px-4 py-3 outline-none focus-visible:ring-2 focus-visible:ring-ring">
                <ChevronRight
                  aria-hidden="true"
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
                />
                <span className="grid gap-0.5 text-sm">
                  <span className="font-medium">{t('provider.advancedSettings')}</span>
                  <span className="text-muted-foreground">
                    {t('provider.advancedSettingsDescription')}
                  </span>
                </span>
              </summary>
              <div className="grid gap-4 border-border border-t px-4 py-4">
                {!isQwen && !isDeepSeek && protocolOptions.length > 1 ? (
                  <label className="grid gap-1 text-sm" htmlFor="provider-protocol">
                    <span className="font-medium">{t('provider.apiMode')}</span>
                    <Select id="provider-protocol" {...register('protocol')}>
                      {protocolOptions.map((option) => (
                        <option key={option} value={option}>
                          {protocolLabel(option)}
                        </option>
                      ))}
                    </Select>
                  </label>
                ) : null}

                {isQwen ? (
                  <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
                    <label className="grid gap-1" htmlFor="provider-protocol">
                      <span className="font-medium">{t('provider.apiMode')}</span>
                      <Select id="provider-protocol" {...register('protocol')}>
                        <option value="responses">Responses</option>
                        <option value="chat_completions">Chat Completions</option>
                        <option value="messages">Messages</option>
                        <option value="dashscope">DashScope</option>
                      </Select>
                    </label>
                    <label className="flex items-center gap-2">
                      <input type="checkbox" {...register('enableThinking')} />
                      <span>{t('provider.enableThinking')}</span>
                    </label>
                    <label className="grid gap-1" htmlFor="provider-thinking-budget">
                      <span className="font-medium">{t('provider.thinkingBudget')}</span>
                      <Input
                        id="provider-thinking-budget"
                        inputMode="numeric"
                        {...register('thinkingBudget')}
                      />
                    </label>
                    <label className="flex items-center gap-2">
                      <input type="checkbox" {...register('preserveThinking')} />
                      <span>{t('provider.preserveThinking')}</span>
                    </label>
                    <label className="grid gap-1" htmlFor="provider-reasoning-effort">
                      <span className="font-medium">{t('provider.reasoningEffort')}</span>
                      <Select id="provider-reasoning-effort" {...register('reasoningEffort')}>
                        <option value="">{t('provider.default')}</option>
                        <option value="none">None</option>
                        <option value="minimal">Minimal</option>
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

                {isDeepSeek ? (
                  <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
                    <label className="grid gap-1" htmlFor="provider-protocol">
                      <span className="font-medium">{t('provider.apiMode')}</span>
                      <Select
                        id="provider-protocol"
                        value={protocol}
                        onChange={(event) => {
                          const nextProtocol = event.currentTarget.value as ModelProtocol
                          setValue('protocol', nextProtocol)
                          setValue('baseUrl', deepseekBaseUrlForProtocol(nextProtocol))
                        }}
                      >
                        <option value="chat_completions">Chat Completions</option>
                        <option value="messages">Anthropic Messages</option>
                      </Select>
                    </label>
                    <label className="grid gap-1" htmlFor="provider-thinking-mode">
                      <span className="font-medium">{t('provider.enableThinking')}</span>
                      <Select id="provider-thinking-mode" {...register('thinkingMode')}>
                        <option value="">{t('provider.default')}</option>
                        <option value="enabled">Enabled</option>
                        <option value="disabled">Disabled</option>
                      </Select>
                    </label>
                    <label className="grid gap-1" htmlFor="provider-reasoning-effort">
                      <span className="font-medium">{t('provider.reasoningEffort')}</span>
                      <Select id="provider-reasoning-effort" {...register('reasoningEffort')}>
                        <option value="">{t('provider.default')}</option>
                        <option value="high">High</option>
                        <option value="max">Max</option>
                      </Select>
                    </label>
                    {thinkingMode === 'disabled' ? (
                      <>
                        <label className="grid gap-1" htmlFor="provider-top-p">
                          <span className="font-medium">{t('provider.topP')}</span>
                          <Input id="provider-top-p" inputMode="decimal" {...register('topP')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-stop-sequences">
                          <span className="font-medium">{t('provider.stopSequences')}</span>
                          <Input id="provider-stop-sequences" {...register('stopSequences')} />
                        </label>
                      </>
                    ) : null}
                  </div>
                ) : null}

                {isOpenAI ? (
                  <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
                    <span className="font-medium">OpenAI Responses options</span>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <label className="grid gap-1" htmlFor="provider-openai-reasoning-effort">
                        <span className="font-medium">OpenAI reasoning effort</span>
                        <Select
                          id="provider-openai-reasoning-effort"
                          {...register('openaiReasoningEffort')}
                        >
                          <option value="">{t('provider.default')}</option>
                          <option value="minimal">minimal</option>
                          <option value="low">low</option>
                          <option value="medium">medium</option>
                          <option value="high">high</option>
                        </Select>
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-reasoning-summary">
                        <span className="font-medium">OpenAI reasoning summary</span>
                        <Select
                          id="provider-openai-reasoning-summary"
                          {...register('openaiReasoningSummary')}
                        >
                          <option value="">{t('provider.default')}</option>
                          <option value="auto">auto</option>
                          <option value="concise">concise</option>
                          <option value="detailed">detailed</option>
                        </Select>
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-reasoning-context">
                        <span className="font-medium">OpenAI reasoning context</span>
                        <Input
                          id="provider-openai-reasoning-context"
                          {...register('openaiReasoningContext')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-text-verbosity">
                        <span className="font-medium">OpenAI text verbosity</span>
                        <Select
                          id="provider-openai-text-verbosity"
                          {...register('openaiTextVerbosity')}
                        >
                          <option value="">{t('provider.default')}</option>
                          <option value="low">low</option>
                          <option value="medium">medium</option>
                          <option value="high">high</option>
                        </Select>
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-service-tier">
                        <span className="font-medium">OpenAI service tier</span>
                        <Select
                          id="provider-openai-service-tier"
                          {...register('openaiServiceTier')}
                        >
                          <option value="">{t('provider.default')}</option>
                          <option value="auto">auto</option>
                          <option value="default">default</option>
                          <option value="flex">flex</option>
                          <option value="priority">priority</option>
                        </Select>
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-truncation">
                        <span className="font-medium">OpenAI truncation</span>
                        <Select id="provider-openai-truncation" {...register('openaiTruncation')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="auto">auto</option>
                          <option value="disabled">disabled</option>
                        </Select>
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-prompt-cache-key">
                        <span className="font-medium">OpenAI prompt cache key</span>
                        <Input
                          id="provider-openai-prompt-cache-key"
                          {...register('openaiPromptCacheKey')}
                        />
                      </label>
                      <label
                        className="grid gap-1"
                        htmlFor="provider-openai-prompt-cache-retention"
                      >
                        <span className="font-medium">OpenAI prompt cache retention</span>
                        <Input
                          id="provider-openai-prompt-cache-retention"
                          {...register('openaiPromptCacheRetention')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-instructions">
                        <span className="font-medium">OpenAI instructions</span>
                        <Input
                          id="provider-openai-instructions"
                          {...register('openaiInstructions')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-include">
                        <span className="font-medium">OpenAI include</span>
                        <Input id="provider-openai-include" {...register('openaiInclude')} />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-max-tool-calls">
                        <span className="font-medium">OpenAI max tool calls</span>
                        <Input
                          id="provider-openai-max-tool-calls"
                          inputMode="numeric"
                          {...register('openaiMaxToolCalls')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-top-logprobs">
                        <span className="font-medium">OpenAI top logprobs</span>
                        <Input
                          id="provider-openai-top-logprobs"
                          inputMode="numeric"
                          {...register('openaiTopLogprobs')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-top-p">
                        <span className="font-medium">OpenAI top P</span>
                        <Input
                          id="provider-openai-top-p"
                          inputMode="decimal"
                          {...register('openaiTopP')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-user">
                        <span className="font-medium">OpenAI user</span>
                        <Input id="provider-openai-user" {...register('openaiUser')} />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-safety-identifier">
                        <span className="font-medium">OpenAI safety identifier</span>
                        <Input
                          id="provider-openai-safety-identifier"
                          {...register('openaiSafetyIdentifier')}
                        />
                      </label>
                    </div>
                    <div className="grid gap-2">
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('openaiBackground')} />
                        <span>OpenAI background mode</span>
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('openaiStore')} />
                        <span>OpenAI store response</span>
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('openaiParallelToolCalls')} />
                        <span>OpenAI parallel tool calls</span>
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('openaiStrictToolSchemas')} />
                        <span>OpenAI strict tool schemas</span>
                      </label>
                    </div>
                    <div className="grid gap-3">
                      <label className="grid gap-1" htmlFor="provider-openai-metadata-json">
                        <span className="font-medium">OpenAI metadata JSON</span>
                        <Input
                          id="provider-openai-metadata-json"
                          {...register('openaiMetadataJson')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-conversation-json">
                        <span className="font-medium">OpenAI conversation JSON</span>
                        <Input
                          id="provider-openai-conversation-json"
                          {...register('openaiConversationJson')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-prompt-json">
                        <span className="font-medium">OpenAI prompt JSON</span>
                        <Input id="provider-openai-prompt-json" {...register('openaiPromptJson')} />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-tool-choice-json">
                        <span className="font-medium">OpenAI tool choice JSON</span>
                        <Input
                          id="provider-openai-tool-choice-json"
                          {...register('openaiToolChoiceJson')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-text-format-json">
                        <span className="font-medium">OpenAI text format JSON</span>
                        <Input
                          id="provider-openai-text-format-json"
                          {...register('openaiTextFormatJson')}
                        />
                      </label>
                      <label className="grid gap-1" htmlFor="provider-openai-advanced-json">
                        <span className="font-medium">OpenAI advanced JSON</span>
                        <Input
                          id="provider-openai-advanced-json"
                          {...register('openaiAdvancedJson')}
                        />
                      </label>
                    </div>
                  </div>
                ) : null}

                {!isQwen && !isDeepSeek && !isOpenAI && supportedParameters.size > 0 ? (
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
                    ) : null}
                    {isZhipu && supportedParameters.has('thinking') ? (
                      <label className="grid gap-1" htmlFor="provider-thinking-mode">
                        <span className="font-medium">{t('provider.enableThinking')}</span>
                        <Select id="provider-thinking-mode" {...register('thinkingMode')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="enabled">{t('provider.enabled')}</option>
                          <option value="disabled">{t('provider.disabled')}</option>
                        </Select>
                      </label>
                    ) : null}
                    {!isDoubao &&
                    !isZhipu &&
                    supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) ? (
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('enableThinking')} />
                        <span>{t('provider.enableThinking')}</span>
                      </label>
                    ) : null}
                    {isZhipu && supportedParameters.has('thinking') ? (
                      <label className="grid gap-1" htmlFor="provider-clear-thinking">
                        <span className="font-medium">{t('provider.clearThinking')}</span>
                        <Select id="provider-clear-thinking" {...register('clearThinking')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="true">{t('provider.enabled')}</option>
                          <option value="false">{t('provider.disabled')}</option>
                        </Select>
                      </label>
                    ) : null}
                    {!isDoubao &&
                    !isZhipu &&
                    supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) ? (
                      <label className="grid gap-1" htmlFor="provider-thinking-budget">
                        <span className="font-medium">{t('provider.thinkingBudget')}</span>
                        <Input
                          id="provider-thinking-budget"
                          inputMode="numeric"
                          {...register('thinkingBudget')}
                        />
                      </label>
                    ) : null}
                    {supportedParameters.has('reasoning_effort') ? (
                      <label className="grid gap-1" htmlFor="provider-reasoning-effort">
                        <span className="font-medium">{t('provider.reasoningEffort')}</span>
                        <Select id="provider-reasoning-effort" {...register('reasoningEffort')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="max">Max</option>
                          <option value="xhigh">XHigh</option>
                          <option value="high">High</option>
                          <option value="medium">Medium</option>
                          <option value="low">Low</option>
                          <option value="minimal">Minimal</option>
                          <option value="none">None</option>
                        </Select>
                      </label>
                    ) : null}
                    {supportedParameters.has('do_sample') ? (
                      <label className="grid gap-1" htmlFor="provider-do-sample">
                        <span className="font-medium">{t('provider.doSample')}</span>
                        <Select id="provider-do-sample" {...register('doSample')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="true">{t('provider.enabled')}</option>
                          <option value="false">{t('provider.disabled')}</option>
                        </Select>
                      </label>
                    ) : null}
                    {supportedParameters.has('tool_stream') ? (
                      <label className="grid gap-1" htmlFor="provider-tool-stream">
                        <span className="font-medium">{t('provider.toolStream')}</span>
                        <Select id="provider-tool-stream" {...register('toolStream')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="true">{t('provider.enabled')}</option>
                          <option value="false">{t('provider.disabled')}</option>
                        </Select>
                      </label>
                    ) : null}
                    {supportedParameters.has('thinkingConfig') ? (
                      <label className="grid gap-1" htmlFor="provider-thinking-level">
                        <span className="font-medium">{t('provider.thinkingLevel')}</span>
                        <Select id="provider-thinking-level" {...register('thinkingLevel')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="MINIMAL">Minimal</option>
                          <option value="LOW">Low</option>
                          <option value="MEDIUM">Medium</option>
                          <option value="HIGH">High</option>
                        </Select>
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
                          {isAnthropic ? <option value="xhigh">XHigh</option> : null}
                          {isAnthropic ? <option value="max">Max</option> : null}
                        </Select>
                      </label>
                    ) : null}
                    {isAnthropic ? (
                      <label className="grid gap-1" htmlFor="provider-thinking-type">
                        <span className="font-medium">Thinking type</span>
                        <Select id="provider-thinking-type" {...register('thinkingType')}>
                          <option value="">{t('provider.default')}</option>
                          {(
                            providerCapabilityMetadata?.thinkingModes ?? [
                              'adaptive',
                              'enabled',
                              'disabled',
                            ]
                          ).map((mode) => (
                            <option key={mode} value={mode}>
                              {mode}
                            </option>
                          ))}
                        </Select>
                      </label>
                    ) : null}
                    {isAnthropic ? (
                      <label className="grid gap-1" htmlFor="provider-thinking-display">
                        <span className="font-medium">Thinking display</span>
                        <Select id="provider-thinking-display" {...register('thinkingDisplay')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="summarized">summarized</option>
                          <option value="omitted">omitted</option>
                        </Select>
                      </label>
                    ) : null}
                    {supportsAny(supportedParameters, ['service_tier', 'serviceTier']) ? (
                      <label className="grid gap-1" htmlFor="provider-service-tier">
                        <span className="font-medium">{t('provider.serviceTier')}</span>
                        <Select id="provider-service-tier" {...register('serviceTier')}>
                          <option value="">{t('provider.default')}</option>
                          {selectedProvider?.providerId === 'gemini' ? (
                            <>
                              <option value="unspecified">Unspecified</option>
                              <option value="standard">Standard</option>
                              <option value="flex">Flex</option>
                              <option value="priority">Priority</option>
                            </>
                          ) : (
                            serviceTierOptions.map((tier) => (
                              <option key={tier} value={tier}>
                                {serviceTierLabel(tier)}
                              </option>
                            ))
                          )}
                        </Select>
                      </label>
                    ) : null}
                    {supportedParameters.has('temperature') ? (
                      <label className="grid gap-1" htmlFor="provider-temperature">
                        <span className="font-medium">{t('provider.temperature')}</span>
                        <Input
                          id="provider-temperature"
                          inputMode="decimal"
                          {...register('temperature')}
                        />
                      </label>
                    ) : null}
                    {selectedProvider?.providerId === 'km' && supportedParameters.has('tools') ? (
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('webSearch')} />
                        <span>$web_search</span>
                      </label>
                    ) : null}
                    {supportedParameters.has('prompt_cache_key') ? (
                      <label className="grid gap-1" htmlFor="provider-prompt-cache-key">
                        <span className="font-medium">{t('provider.promptCacheKey')}</span>
                        <Input id="provider-prompt-cache-key" {...register('promptCacheKey')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('safety_identifier') ? (
                      <label className="grid gap-1" htmlFor="provider-safety-identifier">
                        <span className="font-medium">{t('provider.safetyIdentifier')}</span>
                        <Input id="provider-safety-identifier" {...register('safetyIdentifier')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('partial') ? (
                      <>
                        <label className="grid gap-1" htmlFor="provider-kimi-partial-content">
                          <span className="font-medium">{t('provider.kimiPartialContent')}</span>
                          <Input
                            id="provider-kimi-partial-content"
                            {...register('kimiPartialContent')}
                          />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-kimi-partial-name">
                          <span className="font-medium">{t('provider.kimiPartialName')}</span>
                          <Input id="provider-kimi-partial-name" {...register('kimiPartialName')} />
                        </label>
                      </>
                    ) : null}
                    {supportsAny(supportedParameters, ['top_p', 'topP']) ? (
                      <label className="grid gap-1" htmlFor="provider-top-p">
                        <span className="font-medium">{t('provider.topP')}</span>
                        <Input
                          id="provider-top-p"
                          disabled={anthropicSamplingLocked}
                          inputMode="decimal"
                          {...register('topP')}
                        />
                      </label>
                    ) : null}
                    {supportedParameters.has('max_tokens') ? (
                      <label className="grid gap-1" htmlFor="provider-max-tokens">
                        <span className="font-medium">{t('provider.maxTokens')}</span>
                        <Input
                          id="provider-max-tokens"
                          inputMode="numeric"
                          {...register('maxTokens')}
                        />
                      </label>
                    ) : null}
                    {supportsAny(supportedParameters, ['top_k', 'topK']) ? (
                      <label className="grid gap-1" htmlFor="provider-top-k">
                        <span className="font-medium">{t('provider.topK')}</span>
                        <Input
                          id="provider-top-k"
                          disabled={anthropicSamplingLocked}
                          inputMode="numeric"
                          {...register('topK')}
                        />
                      </label>
                    ) : null}
                    {supportedParameters.has('seed') ? (
                      <label className="grid gap-1" htmlFor="provider-seed">
                        <span className="font-medium">{t('provider.seed')}</span>
                        <Input id="provider-seed" inputMode="numeric" {...register('seed')} />
                      </label>
                    ) : null}
                    {supportsAny(supportedParameters, [
                      'stop_sequences',
                      'stopSequences',
                      'stop',
                    ]) ? (
                      <label className="grid gap-1" htmlFor="provider-stop-sequences">
                        <span className="font-medium">{t('provider.stopSequences')}</span>
                        <Input id="provider-stop-sequences" {...register('stopSequences')} />
                      </label>
                    ) : null}
                    {isAnthropic ? (
                      <>
                        <label className="grid gap-1" htmlFor="provider-tool-choice">
                          <span className="font-medium">Tool choice</span>
                          <Select id="provider-tool-choice" {...register('toolChoice')}>
                            <option value="">{t('provider.default')}</option>
                            <option value="auto">auto</option>
                            <option value="none">none</option>
                            <option value="any">any</option>
                            <option value="tool">tool</option>
                          </Select>
                        </label>
                        <label className="grid gap-1" htmlFor="provider-tool-name">
                          <span className="font-medium">Tool name</span>
                          <Input id="provider-tool-name" {...register('toolName')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-cache-ttl">
                          <span className="font-medium">Cache TTL</span>
                          <Select id="provider-cache-ttl" {...register('cacheTtl')}>
                            <option value="">{t('provider.default')}</option>
                            <option value="5m">5m</option>
                            <option value="1h">1h</option>
                          </Select>
                        </label>
                        <label className="grid gap-1" htmlFor="provider-anthropic-beta">
                          <span className="font-medium">Anthropic beta</span>
                          <Input id="provider-anthropic-beta" {...register('anthropicBeta')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-user-profile-id">
                          <span className="font-medium">User profile ID</span>
                          <Input
                            id="provider-user-profile-id"
                            {...register('anthropicUserProfileId')}
                          />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-inference-geo">
                          <span className="font-medium">Inference geo</span>
                          <Input id="provider-inference-geo" {...register('inferenceGeo')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-speed">
                          <span className="font-medium">Speed</span>
                          <Input id="provider-speed" {...register('speed')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-metadata-json">
                          <span className="font-medium">Metadata JSON</span>
                          <Input id="provider-metadata-json" {...register('metadataJson')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-fallbacks-json">
                          <span className="font-medium">Fallbacks JSON</span>
                          <Input id="provider-fallbacks-json" {...register('fallbacksJson')} />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-context-management-json">
                          <span className="font-medium">Context management JSON</span>
                          <Input
                            id="provider-context-management-json"
                            {...register('contextManagementJson')}
                          />
                        </label>
                        <label className="grid gap-1" htmlFor="provider-anthropic-advanced-json">
                          <span className="font-medium">Advanced Anthropic JSON</span>
                          <Input
                            id="provider-anthropic-advanced-json"
                            {...register('anthropicAdvancedJson')}
                          />
                        </label>
                      </>
                    ) : null}
                    {supportedParameters.has('response_format') ? (
                      <label className="grid gap-1" htmlFor="provider-response-format">
                        <span className="font-medium">{t('provider.responseFormat')}</span>
                        <Select id="provider-response-format" {...register('responseFormat')}>
                          <option value="">{t('provider.default')}</option>
                          <option value="json_object">JSON object</option>
                        </Select>
                      </label>
                    ) : null}
                    {supportedParameters.has('responseMimeType') ? (
                      <label className="grid gap-1" htmlFor="provider-response-mime-type">
                        <span className="font-medium">{t('provider.responseMimeType')}</span>
                        <Input id="provider-response-mime-type" {...register('responseMimeType')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('user_id') ? (
                      <label className="grid gap-1" htmlFor="provider-user-id">
                        <span className="font-medium">{t('provider.userId')}</span>
                        <Input id="provider-user-id" {...register('userId')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('responseJsonSchema') ? (
                      <label className="grid gap-1" htmlFor="provider-response-json-schema">
                        <span className="font-medium">{t('provider.responseJsonSchema')}</span>
                        <Textarea
                          id="provider-response-json-schema"
                          rows={3}
                          {...register('responseJsonSchema')}
                        />
                      </label>
                    ) : null}
                    {supportedParameters.has('toolConfig') ? (
                      <label className="grid gap-1" htmlFor="provider-tool-config">
                        <span className="font-medium">{t('provider.toolConfig')}</span>
                        <Textarea id="provider-tool-config" rows={3} {...register('toolConfig')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('safetySettings') ? (
                      <label className="grid gap-1" htmlFor="provider-safety-settings">
                        <span className="font-medium">{t('provider.safetySettings')}</span>
                        <Textarea
                          id="provider-safety-settings"
                          rows={3}
                          {...register('safetySettings')}
                        />
                      </label>
                    ) : null}
                    {supportedParameters.has('cachedContent') ? (
                      <label className="grid gap-1" htmlFor="provider-cached-content">
                        <span className="font-medium">{t('provider.cachedContent')}</span>
                        <Input id="provider-cached-content" {...register('cachedContent')} />
                      </label>
                    ) : null}
                    {supportedParameters.has('store') ? (
                      <label className="flex items-center gap-2">
                        <input type="checkbox" {...register('storeResponse')} />
                        <span>{t('provider.storeResponse')}</span>
                      </label>
                    ) : null}
                    {supportedParameters.has('performanceConfig') ? (
                      <label className="grid gap-1" htmlFor="provider-performance-latency">
                        <span className="font-medium">{t('provider.performanceLatency')}</span>
                        <Select
                          id="provider-performance-latency"
                          {...register('performanceLatency')}
                        >
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
              </div>
            </details>

            {errors.root?.message ? (
              <p className="text-destructive text-sm" role="alert">
                {errors.root.message}
              </p>
            ) : null}
          </div>

          <DialogFooter className="border-border border-t px-6 py-4">
            <Button
              disabled={isSubmitting}
              type="button"
              variant="outline"
              onClick={() => changeOpen(false)}
            >
              {t('models.configDialog.cancel')}
            </Button>
            <Button disabled={isSubmitting || !selectedModelIsRunnable} type="submit">
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
  const openaiDefaults = openAiResponsesDefaultsFromProfile(profile)
  const kimiDefaults = kimiDefaultsFromProfile(profile)
  return {
    advancedBodyJson: advancedDefaults.body,
    baseUrl: profile?.baseUrl ?? defaultProvider?.defaultBaseUrl ?? '',
    anthropicAdvancedJson: providerDefaults.anthropicAdvancedJson,
    anthropicBeta: providerDefaults.anthropicBeta,
    anthropicUserProfileId: providerDefaults.anthropicUserProfileId,
    cacheTtl: providerDefaults.cacheTtl,
    clearThinking: providerDefaults.clearThinking,
    codeInterpreter: defaults.codeInterpreter,
    contextManagementJson: providerDefaults.contextManagementJson,
    displayName: profile?.displayName ?? '',
    doSample: providerDefaults.doSample,
    enableThinking: defaults.enableThinking || providerDefaults.enableThinking,
    fallbacksJson: providerDefaults.fallbacksJson,
    inferenceGeo: providerDefaults.inferenceGeo,
    metadataJson: providerDefaults.metadataJson,
    maxTokens: providerDefaults.maxTokens,
    kimiPartialContent: kimiDefaults.kimiPartialContent,
    kimiPartialName: kimiDefaults.kimiPartialName,
    modelId: profile?.modelId ?? defaultModel?.modelId ?? '',
    openaiAdvancedJson: openaiDefaults.advancedJson,
    openaiBackground: openaiDefaults.background,
    openaiConversationJson: openaiDefaults.conversationJson,
    openaiInclude: openaiDefaults.include,
    openaiInstructions: openaiDefaults.instructions,
    openaiMaxToolCalls: openaiDefaults.maxToolCalls,
    openaiMetadataJson: openaiDefaults.metadataJson,
    openaiParallelToolCalls: openaiDefaults.parallelToolCalls,
    openaiPromptCacheKey: openaiDefaults.promptCacheKey,
    openaiPromptCacheRetention: openaiDefaults.promptCacheRetention,
    openaiPromptJson: openaiDefaults.promptJson,
    openaiReasoningContext: openaiDefaults.reasoningContext,
    openaiReasoningEffort: openaiDefaults.reasoningEffort,
    openaiReasoningSummary: openaiDefaults.reasoningSummary,
    openaiSafetyIdentifier: openaiDefaults.safetyIdentifier,
    openaiServiceTier: openaiDefaults.serviceTier,
    openaiStore: openaiDefaults.store,
    openaiStrictToolSchemas: openaiDefaults.strictToolSchemas,
    openaiTextFormatJson: openaiDefaults.textFormatJson,
    openaiTextVerbosity: openaiDefaults.textVerbosity,
    openaiToolChoiceJson: openaiDefaults.toolChoiceJson,
    openaiTopLogprobs: openaiDefaults.topLogprobs,
    openaiTopP: openaiDefaults.topP,
    openaiTruncation: openaiDefaults.truncation,
    openaiUser: openaiDefaults.user,
    outputEffort: providerDefaults.outputEffort,
    performanceLatency: providerDefaults.performanceLatency,
    promptCacheKey: providerDefaults.promptCacheKey,
    preserveThinking: defaults.preserveThinking,
    protocol: profile?.protocol ?? defaultProtocolForProvider(defaultProvider),
    providerId: profile?.providerId ?? defaultProvider?.providerId ?? '',
    reasoningEffort: defaults.reasoningEffort || providerDefaults.reasoningEffort,
    responseFormat: providerDefaults.responseFormat,
    cachedContent: providerDefaults.cachedContent,
    responseMimeType: providerDefaults.responseMimeType,
    responseJsonSchema: providerDefaults.responseJsonSchema,
    seed: providerDefaults.seed,
    serviceTier: providerDefaults.serviceTier,
    safetyIdentifier: providerDefaults.safetyIdentifier,
    sessionCache: defaults.sessionCache,
    stopSequences: providerDefaults.stopSequences,
    thinkingMode: providerDefaults.thinkingMode,
    thinkingBudget: providerDefaults.thinkingBudget,
    thinkingDisplay: providerDefaults.thinkingDisplay,
    thinkingType: providerDefaults.thinkingType,
    temperature: providerDefaults.temperature,
    toolStream: providerDefaults.toolStream,
    safetySettings: providerDefaults.safetySettings,
    storeResponse: providerDefaults.storeResponse,
    thinkingLevel: providerDefaults.thinkingLevel,
    toolConfig: providerDefaults.toolConfig,
    topK: providerDefaults.topK,
    topP: providerDefaults.topP,
    toolChoice: providerDefaults.toolChoice,
    toolName: providerDefaults.toolName,
    speed: providerDefaults.speed,
    userId: providerDefaults.userId,
    webExtractor: defaults.webExtractor,
    webSearch: defaults.webSearch || providerDefaults.webSearch,
  }
}

function defaultProtocolForProvider(
  provider: ModelProviderCatalogResponse['providers'][number] | undefined,
): ModelProtocol {
  return defaultProtocolForModel(firstRunnableModel(provider))
}

function firstRunnableModel(
  provider: ModelProviderCatalogResponse['providers'][number] | undefined,
) {
  return provider?.models.find((model) => model.runtimeStatus.kind === 'runnable')
}

function defaultProtocolForModel(
  model: ModelProviderCatalogResponse['providers'][number]['models'][number] | undefined,
): ModelProtocol {
  return model?.supportedProtocols?.[0] ?? model?.protocol ?? 'responses'
}

function providerPersistsProtocol(providerId: string) {
  return providerId === 'qwen' || providerId === 'deepseek' || providerId === 'minimax'
}

function protocolLabel(protocol: ModelProtocol): string {
  switch (protocol) {
    case 'chat_completions':
      return 'Chat Completions'
    case 'messages':
      return 'Messages'
    case 'responses':
      return 'Responses'
    default:
      return protocol
  }
}

function serviceTierLabel(tier: string): string {
  return tier
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
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
  const thinking = isRecord(body.thinking) ? body.thinking : null
  const reasoningEffort = typeof reasoning?.effort === 'string' ? reasoning.effort : ''
  const searchOptions = isRecord(body.search_options) ? body.search_options : null
  const qwenChatWebExtractor = searchOptions?.search_strategy === 'agent_max'
  const headers = profile?.providerDefaults?.headers ?? {}
  const sessionCache = Object.entries(headers).some(
    ([name, value]) => name.toLowerCase() === 'x-dashscope-session-cache' && value === 'enable',
  )

  return {
    codeInterpreter: toolTypes.has('code_interpreter') || body.enable_code_interpreter === true,
    enableThinking: body.enable_thinking === true || thinking !== null,
    preserveThinking: body.preserve_thinking === true,
    reasoningEffort,
    sessionCache,
    webExtractor: toolTypes.has('web_extractor') || qwenChatWebExtractor,
    webSearch: toolTypes.has('web_search') || body.enable_search === true,
  }
}

function providerOptionDefaultsFromProfile(profile: ProviderConfig | null | undefined) {
  const body = profile?.providerDefaults?.body ?? {}
  const tools = Array.isArray(body.tools) ? body.tools : []
  const thinking = isRecord(body.thinking) ? body.thinking : null
  const thinkingConfig = isRecord(body.thinkingConfig) ? body.thinkingConfig : null
  const outputConfig = isRecord(body.output_config) ? body.output_config : null
  const inferenceConfig = isRecord(body.inferenceConfig) ? body.inferenceConfig : null
  const performanceConfig = isRecord(body.performanceConfig) ? body.performanceConfig : null
  const cacheControl = isRecord(body.cache_control) ? body.cache_control : null
  const toolChoice = isRecord(body.tool_choice) ? body.tool_choice : null
  const topP = firstStringable(body.top_p, body.topP, inferenceConfig?.topP)
  const topK = firstStringable(body.top_k, body.topK)
  const stopSequences = firstArray(
    body.stop,
    body.stop_sequences,
    body.stopSequences,
    inferenceConfig?.stopSequences,
  )

  return {
    clearThinking:
      typeof thinking?.clear_thinking === 'boolean' ? String(thinking.clear_thinking) : '',
    doSample: typeof body.do_sample === 'boolean' ? String(body.do_sample) : '',
    enableThinking:
      thinking !== null ||
      thinkingConfig !== null ||
      body.enable_thinking === true ||
      body.enableThinking === true,
    anthropicAdvancedJson: '',
    anthropicBeta: headerValue(profile, 'anthropic-beta'),
    anthropicUserProfileId: headerValue(profile, 'anthropic-user-profile-id'),
    cacheTtl: typeof cacheControl?.ttl === 'string' ? cacheControl.ttl : '',
    contextManagementJson: jsonField(body.context_management),
    fallbacksJson: jsonField(body.fallbacks),
    inferenceGeo: typeof body.inference_geo === 'string' ? body.inference_geo : '',
    metadataJson: jsonField(body.metadata),
    outputEffort: typeof outputConfig?.effort === 'string' ? outputConfig.effort : '',
    performanceLatency:
      typeof performanceConfig?.latency === 'string' ? performanceConfig.latency : '',
    reasoningEffort:
      typeof body.reasoning_effort === 'string'
        ? body.reasoning_effort
        : typeof outputConfig?.effort === 'string'
          ? outputConfig.effort
          : '',
    maxTokens: firstStringable(body.max_tokens, body.max_completion_tokens),
    responseFormat:
      isRecord(body.response_format) && typeof body.response_format.type === 'string'
        ? body.response_format.type
        : '',
    promptCacheKey: typeof body.prompt_cache_key === 'string' ? body.prompt_cache_key : '',
    responseMimeType: typeof body.responseMimeType === 'string' ? body.responseMimeType : '',
    safetyIdentifier: typeof body.safety_identifier === 'string' ? body.safety_identifier : '',
    cachedContent:
      typeof body.cachedContent === 'string'
        ? body.cachedContent
        : typeof body.cached_content === 'string'
          ? body.cached_content
          : '',
    responseJsonSchema: jsonText(body.responseJsonSchema),
    seed: firstStringable(body.seed),
    serviceTier:
      typeof body.service_tier === 'string'
        ? body.service_tier
        : typeof body.serviceTier === 'string'
          ? body.serviceTier
          : '',
    safetySettings: Array.isArray(body.safetySettings) ? JSON.stringify(body.safetySettings) : '',
    stopSequences: stopSequences.join(','),
    thinkingMode:
      thinking?.type === 'enabled' || thinking?.type === 'disabled' ? thinking.type : '',
    thinkingBudget: firstStringable(
      body.thinking_budget,
      thinking?.budget_tokens,
      thinkingConfig?.thinkingBudget,
    ),
    thinkingDisplay: typeof thinking?.display === 'string' ? thinking.display : '',
    thinkingType: typeof thinking?.type === 'string' ? thinking.type : '',
    storeResponse: body.store === true,
    thinkingLevel:
      typeof thinkingConfig?.thinkingLevel === 'string' ? thinkingConfig.thinkingLevel : '',
    toolConfig: jsonText(body.toolConfig),
    topK,
    topP,
    toolChoice: typeof toolChoice?.type === 'string' ? toolChoice.type : '',
    toolName: typeof toolChoice?.name === 'string' ? toolChoice.name : '',
    speed: typeof body.speed === 'string' ? body.speed : '',
    temperature: firstStringable(body.temperature),
    toolStream: typeof body.tool_stream === 'boolean' ? String(body.tool_stream) : '',
    userId: typeof body.user_id === 'string' ? body.user_id : '',
    webSearch: tools.some(isKimiWebSearchTool),
  }
}

function kimiDefaultsFromProfile(profile: ProviderConfig | null | undefined) {
  const kimiChat = profile?.modelOptions?.kimiChat
  const partial = kimiChat?.partialAssistant
  return {
    kimiPartialContent: partial?.content ?? '',
    kimiPartialName: partial?.name ?? '',
  }
}

function openAiResponsesDefaultsFromProfile(profile: ProviderConfig | null | undefined) {
  const options = profile?.modelOptions?.openaiResponses
  const reasoning = isRecord(options?.reasoning) ? options.reasoning : null
  const text = isRecord(options?.text) ? options.text : null

  return {
    advancedJson: '',
    background: options?.background === true,
    conversationJson: jsonField(options?.conversation),
    include: Array.isArray(options?.include) ? options.include.join(',') : '',
    instructions: typeof options?.instructions === 'string' ? options.instructions : '',
    maxToolCalls: firstStringable(options?.maxToolCalls),
    metadataJson: jsonField(options?.metadata),
    parallelToolCalls: options?.parallelToolCalls === true,
    promptCacheKey: typeof options?.promptCacheKey === 'string' ? options.promptCacheKey : '',
    promptCacheRetention:
      typeof options?.promptCacheRetention === 'string' ? options.promptCacheRetention : '',
    promptJson: jsonField(options?.prompt),
    reasoningContext: typeof reasoning?.context === 'string' ? reasoning.context : '',
    reasoningEffort: typeof reasoning?.effort === 'string' ? reasoning.effort : '',
    reasoningSummary: typeof reasoning?.summary === 'string' ? reasoning.summary : '',
    safetyIdentifier: typeof options?.safetyIdentifier === 'string' ? options.safetyIdentifier : '',
    serviceTier: typeof options?.serviceTier === 'string' ? options.serviceTier : '',
    store: options?.store === true,
    strictToolSchemas: options?.strictToolSchemas === true,
    textFormatJson: jsonField(text?.format),
    textVerbosity: typeof text?.verbosity === 'string' ? text.verbosity : '',
    toolChoiceJson: jsonField(options?.toolChoice),
    topLogprobs: firstStringable(options?.topLogprobs),
    topP: firstStringable(options?.topP),
    truncation: typeof options?.truncation === 'string' ? options.truncation : '',
    user: typeof options?.user === 'string' ? options.user : '',
  }
}

function openAiResponsesOptionsFromValues(values: ModelConfigFormValues): OpenAiResponsesOptions {
  const options: OpenAiResponsesOptions = {}
  const reasoning: NonNullable<OpenAiResponsesOptions['reasoning']> = {}
  const text: NonNullable<OpenAiResponsesOptions['text']> = {}
  const include = parseList(values.openaiInclude)
  const maxToolCalls = parseNumber(values.openaiMaxToolCalls)
  const topLogprobs = parseNumber(values.openaiTopLogprobs)
  const topP = parseNumber(values.openaiTopP)

  if (values.openaiBackground) {
    options.background = true
  }
  if (values.openaiConversationJson.trim()) {
    options.conversation = parseJsonValue(values.openaiConversationJson, 'OpenAI conversation JSON')
  }
  if (include.length > 0) {
    options.include = include
  }
  if (values.openaiInstructions.trim()) {
    options.instructions = values.openaiInstructions.trim()
  }
  if (maxToolCalls !== null) {
    options.maxToolCalls = maxToolCalls
  }
  if (values.openaiPromptJson.trim()) {
    options.prompt = parseJsonValue(values.openaiPromptJson, 'OpenAI prompt JSON')
  }
  if (values.openaiPromptCacheKey.trim()) {
    options.promptCacheKey = values.openaiPromptCacheKey.trim()
  }
  if (values.openaiPromptCacheRetention.trim()) {
    options.promptCacheRetention = values.openaiPromptCacheRetention.trim()
  }
  if (values.openaiReasoningEffort) {
    reasoning.effort = values.openaiReasoningEffort
  }
  if (values.openaiReasoningSummary) {
    reasoning.summary = values.openaiReasoningSummary
  }
  if (values.openaiReasoningContext.trim()) {
    reasoning.context = values.openaiReasoningContext.trim()
  }
  if (Object.keys(reasoning).length > 0) {
    options.reasoning = reasoning
  }
  if (values.openaiSafetyIdentifier.trim()) {
    options.safetyIdentifier = values.openaiSafetyIdentifier.trim()
  }
  if (values.openaiServiceTier) {
    options.serviceTier = values.openaiServiceTier
  }
  if (values.openaiTextVerbosity) {
    text.verbosity = values.openaiTextVerbosity
  }
  if (values.openaiTextFormatJson.trim()) {
    text.format = parseJsonObject(
      values.openaiTextFormatJson,
      'OpenAI text format JSON',
    ) as NonNullable<OpenAiResponsesOptions['text']>['format']
  }
  if (Object.keys(text).length > 0) {
    options.text = text
  }
  if (topLogprobs !== null) {
    options.topLogprobs = topLogprobs
  }
  if (topP !== null) {
    options.topP = topP
  }
  if (values.openaiToolChoiceJson.trim()) {
    options.toolChoice = parseJsonValue(values.openaiToolChoiceJson, 'OpenAI tool choice JSON')
  }
  if (values.openaiParallelToolCalls) {
    options.parallelToolCalls = true
  }
  if (values.openaiTruncation) {
    options.truncation = values.openaiTruncation
  }
  if (values.openaiStore) {
    options.store = true
  }
  if (values.openaiMetadataJson.trim()) {
    options.metadata = parseStringRecord(values.openaiMetadataJson, 'OpenAI metadata JSON')
  }
  if (values.openaiUser.trim()) {
    options.user = values.openaiUser.trim()
  }
  if (values.openaiStrictToolSchemas) {
    options.strictToolSchemas = true
  }
  if (values.openaiAdvancedJson.trim()) {
    mergeAdvancedOpenAiResponsesOptions(options, values.openaiAdvancedJson)
  }
  return options
}

function hasOpenAiResponsesOptions(options: OpenAiResponsesOptions): boolean {
  return Object.keys(options).length > 0
}

function hasModelOptions(options: ProviderSettingsRequest['modelOptions']): boolean {
  return Object.keys(options ?? {}).length > 0
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
    const temperature = parseNumber(values.temperature)
    const maxTokens = parseNumber(values.maxTokens)

    if (values.providerId === 'anthropic') {
      const thinkingType = values.thinkingType || (values.enableThinking ? 'enabled' : '')
      if (thinkingType) {
        body.thinking = {
          type: thinkingType,
          ...(thinkingBudget !== null ? { budget_tokens: thinkingBudget } : {}),
          ...(values.thinkingDisplay ? { display: values.thinkingDisplay } : {}),
        }
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
      if (values.toolChoice) {
        if (values.toolChoice === 'tool' && !values.toolName.trim()) {
          throw new Error('Tool name is required when tool_choice is tool')
        }
        body.tool_choice =
          values.toolChoice === 'tool'
            ? { type: 'tool', name: values.toolName.trim() }
            : { type: values.toolChoice }
      }
      if (values.cacheTtl) {
        body.cache_control = { type: 'ephemeral', ttl: values.cacheTtl }
      }
      if (values.metadataJson.trim()) {
        body.metadata = parseJsonObject(values.metadataJson, 'Metadata JSON')
      }
      if (values.inferenceGeo.trim()) {
        body.inference_geo = values.inferenceGeo.trim()
      }
      if (values.speed.trim()) {
        body.speed = values.speed.trim()
      }
      if (values.fallbacksJson.trim()) {
        body.fallbacks = parseJsonValue(values.fallbacksJson, 'Fallbacks JSON')
      }
      if (values.contextManagementJson.trim()) {
        body.context_management = parseJsonValue(
          values.contextManagementJson,
          'Context management JSON',
        )
      }
      if (values.anthropicBeta.trim()) {
        headers['anthropic-beta'] = values.anthropicBeta.trim()
      }
      if (values.anthropicUserProfileId.trim()) {
        headers['anthropic-user-profile-id'] = values.anthropicUserProfileId.trim()
      }
      if (values.anthropicAdvancedJson.trim()) {
        mergeAdvancedAnthropicBody(body, values.anthropicAdvancedJson)
      }
      return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
    }

    if (values.providerId === 'gemini') {
      if (values.enableThinking || thinkingBudget !== null || values.thinkingLevel) {
        body.thinkingConfig = {
          ...(values.enableThinking ? { includeThoughts: true } : {}),
          ...(thinkingBudget !== null ? { thinkingBudget } : {}),
          ...(values.thinkingLevel ? { thinkingLevel: values.thinkingLevel } : {}),
        }
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
      const responseJsonSchema = parseJsonField(values.responseJsonSchema)
      if (responseJsonSchema !== null) {
        body.responseJsonSchema = responseJsonSchema
      }
      const toolConfig = parseJsonField(values.toolConfig)
      if (toolConfig !== null) {
        body.toolConfig = toolConfig
      }
      const safetySettings = parseJsonField(values.safetySettings)
      if (Array.isArray(safetySettings)) {
        body.safetySettings = safetySettings
      }
      if (values.cachedContent.trim()) {
        body.cachedContent = values.cachedContent.trim()
      }
      if (values.serviceTier) {
        body.serviceTier = values.serviceTier
      }
      if (values.storeResponse) {
        body.store = true
      }
      return { body, headers }
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

    if (values.providerId === 'deepseek') {
      if (values.thinkingMode === 'enabled' || values.thinkingMode === 'disabled') {
        body.thinking = { type: values.thinkingMode }
      }
      if (values.thinkingMode === 'disabled' && topP !== null) {
        body.top_p = topP
      }
      if (values.protocol === 'messages') {
        if (values.reasoningEffort === 'high' || values.reasoningEffort === 'max') {
          body.output_config = { effort: values.reasoningEffort }
        }
        if (stopSequences.length > 0) {
          body.stop_sequences = stopSequences
        }
      } else {
        if (values.reasoningEffort === 'high' || values.reasoningEffort === 'max') {
          body.reasoning_effort = values.reasoningEffort
        }
        if (stopSequences.length > 0) {
          body.stop = stopSequences
        }
      }
      return { body, headers }
    }

    if (values.providerId === 'zhipu') {
      if (values.thinkingMode || values.clearThinking) {
        body.thinking = {
          ...(values.thinkingMode ? { type: values.thinkingMode } : {}),
          ...(values.clearThinking ? { clear_thinking: values.clearThinking === 'true' } : {}),
        }
      }
      if (values.reasoningEffort) {
        body.reasoning_effort = values.reasoningEffort
      }
      if (values.doSample === 'true') {
        body.do_sample = true
      } else if (values.doSample === 'false') {
        body.do_sample = false
      }
      if (values.toolStream === 'true') {
        body.tool_stream = true
      } else if (values.toolStream === 'false') {
        body.tool_stream = false
      }
      if (temperature !== null) {
        body.temperature = temperature
      }
      if (topP !== null) {
        body.top_p = topP
      }
      if (maxTokens !== null) {
        body.max_tokens = maxTokens
      }
      if (stopSequences.length > 0) {
        body.stop = stopSequences
      }
      if (values.responseFormat) {
        body.response_format = { type: values.responseFormat }
      }
      if (values.userId.trim()) {
        body.user_id = values.userId.trim()
      }
      return mergeAdvancedProviderDefaults(body, headers, values, supportedParameters)
    }

    if (values.providerId === 'km') {
      if (topP !== null) {
        body.top_p = topP
      }
      if (stopSequences.length > 0) {
        body.stop = stopSequences
      }
      if (values.enableThinking) {
        body.thinking = { type: 'enabled' }
      }
      if (values.webSearch) {
        body.tools = [
          {
            type: 'builtin_function',
            function: { name: '$web_search' },
          },
        ]
      }
      if (values.promptCacheKey.trim()) {
        body.prompt_cache_key = values.promptCacheKey.trim()
      }
      if (values.safetyIdentifier.trim()) {
        body.safety_identifier = values.safetyIdentifier.trim()
      }
      return { body, headers }
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
    if (values.protocol === 'messages') {
      const thinkingBudget = parseNumber(values.thinkingBudget)
      body.thinking =
        thinkingBudget !== null
          ? { type: 'enabled', budget_tokens: thinkingBudget }
          : { type: 'enabled' }
    } else {
      body.enable_thinking = true
    }
  }
  const qwenThinkingBudget = parseNumber(values.thinkingBudget)
  if (values.protocol !== 'messages' && qwenThinkingBudget !== null) {
    body.thinking_budget = qwenThinkingBudget
  }
  if (values.protocol !== 'messages' && values.preserveThinking) {
    body.preserve_thinking = true
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
  } else if (values.protocol === 'chat_completions' || values.protocol === 'dashscope') {
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

function modelOptionsFromValues(
  values: ModelConfigFormValues,
): ProviderSettingsRequest['modelOptions'] {
  if (values.providerId !== 'km') {
    return {}
  }

  const kimiChat: NonNullable<ProviderSettingsRequest['modelOptions']>['kimiChat'] = {}
  const partialContent = values.kimiPartialContent.trim()
  if (partialContent) {
    const partialAssistant: NonNullable<typeof kimiChat>['partialAssistant'] = {
      content: partialContent,
    }
    if (values.kimiPartialName.trim()) {
      partialAssistant.name = values.kimiPartialName.trim()
    }
    kimiChat.partialAssistant = partialAssistant
  }

  return Object.keys(kimiChat).length > 0 ? { kimiChat } : {}
}

function hasProviderDefaults(defaults: ProviderSettingsRequest['providerDefaults']): boolean {
  return (
    Object.keys(defaults?.body ?? {}).length > 0 || Object.keys(defaults?.headers ?? {}).length > 0
  )
}

function resetProviderOptionFields(setValue: UseFormSetValue<ModelConfigFormValues>) {
  setValue('advancedBodyJson', '')
  setValue('clearThinking', '')
  setValue('doSample', '')
  setValue('enableThinking', false)
  setValue('thinkingType', '')
  setValue('thinkingDisplay', '')
  setValue('cacheTtl', '')
  setValue('maxTokens', '')
  setValue('kimiPartialContent', '')
  setValue('kimiPartialName', '')
  setValue('outputEffort', '')
  setValue('performanceLatency', '')
  setValue('promptCacheKey', '')
  setValue('preserveThinking', false)
  setValue('reasoningEffort', '')
  setValue('responseFormat', '')
  setValue('responseMimeType', '')
  setValue('safetyIdentifier', '')
  setValue('cachedContent', '')
  setValue('responseJsonSchema', '')
  setValue('seed', '')
  setValue('serviceTier', '')
  setValue('safetySettings', '')
  setValue('stopSequences', '')
  setValue('thinkingMode', '')
  setValue('temperature', '')
  setValue('thinkingBudget', '')
  setValue('toolStream', '')
  setValue('storeResponse', false)
  setValue('thinkingLevel', '')
  setValue('toolConfig', '')
  setValue('topK', '')
  setValue('topP', '')
  setValue('toolChoice', '')
  setValue('toolName', '')
  setValue('metadataJson', '')
  setValue('anthropicBeta', '')
  setValue('anthropicUserProfileId', '')
  setValue('inferenceGeo', '')
  setValue('speed', '')
  setValue('fallbacksJson', '')
  setValue('contextManagementJson', '')
  setValue('anthropicAdvancedJson', '')
  setValue('userId', '')
  setValue('openaiAdvancedJson', '')
  setValue('openaiBackground', false)
  setValue('openaiConversationJson', '')
  setValue('openaiInclude', '')
  setValue('openaiInstructions', '')
  setValue('openaiMaxToolCalls', '')
  setValue('openaiMetadataJson', '')
  setValue('openaiParallelToolCalls', false)
  setValue('openaiPromptCacheKey', '')
  setValue('openaiPromptCacheRetention', '')
  setValue('openaiPromptJson', '')
  setValue('openaiReasoningContext', '')
  setValue('openaiReasoningEffort', '')
  setValue('openaiReasoningSummary', '')
  setValue('openaiSafetyIdentifier', '')
  setValue('openaiServiceTier', '')
  setValue('openaiStore', false)
  setValue('openaiStrictToolSchemas', false)
  setValue('openaiTextFormatJson', '')
  setValue('openaiTextVerbosity', '')
  setValue('openaiToolChoiceJson', '')
  setValue('openaiTopLogprobs', '')
  setValue('openaiTopP', '')
  setValue('openaiTruncation', '')
  setValue('openaiUser', '')
  setValue('webSearch', false)
  setValue('codeInterpreter', false)
  setValue('webExtractor', false)
  setValue('sessionCache', false)
}

type AnthropicCapabilityMetadata = {
  protocolSupportedParameters?: Partial<Record<ModelProtocol, string[]>>
  serviceTiers?: string[]
  thinkingModes?: string[]
  samplingLocked?: boolean
}

function getAnthropicCapabilityMetadata(value: unknown): AnthropicCapabilityMetadata | null {
  if (!isRecord(value)) {
    return null
  }
  return {
    protocolSupportedParameters: isRecord(value.protocolSupportedParameters)
      ? Object.fromEntries(
          Object.entries(value.protocolSupportedParameters).filter(
            ([protocol, parameters]) =>
              (protocol === 'responses' ||
                protocol === 'chat_completions' ||
                protocol === 'messages') &&
              Array.isArray(parameters) &&
              parameters.every((parameter) => typeof parameter === 'string'),
          ),
        )
      : undefined,
    serviceTiers: Array.isArray(value.serviceTiers)
      ? value.serviceTiers.filter((tier): tier is string => typeof tier === 'string')
      : undefined,
    thinkingModes: Array.isArray(value.thinkingModes)
      ? value.thinkingModes.filter((mode): mode is string => typeof mode === 'string')
      : undefined,
    samplingLocked: value.samplingLocked === true,
  }
}

function headerValue(profile: ProviderConfig | null | undefined, name: string): string {
  const headers = profile?.providerDefaults?.headers ?? {}
  const entry = Object.entries(headers).find(([key]) => key.toLowerCase() === name.toLowerCase())
  return entry?.[1] ?? ''
}

function jsonField(value: unknown): string {
  return value === undefined || value === null ? '' : JSON.stringify(value)
}

function parseJsonValue(value: string, label: string): unknown {
  try {
    return JSON.parse(value)
  } catch {
    throw new Error(`${label} must be valid JSON`)
  }
}

function parseJsonObject(value: string, label: string): Record<string, unknown> {
  const parsed = parseJsonValue(value, label)
  if (!isRecord(parsed)) {
    throw new Error(`${label} must be a JSON object`)
  }
  return parsed
}

function parseStringRecord(value: string, label: string): Record<string, string> {
  const parsed = parseJsonObject(value, label)
  const record: Record<string, string> = {}
  for (const [key, fieldValue] of Object.entries(parsed)) {
    if (typeof fieldValue !== 'string') {
      throw new Error(`${label} values must be strings`)
    }
    record[key] = fieldValue
  }
  return record
}

function mergeAdvancedAnthropicBody(body: Record<string, unknown>, value: string) {
  const advanced = parseJsonObject(value, 'Advanced Anthropic JSON')
  for (const forbidden of ['model', 'messages', 'input', 'contents', 'stream', 'max_tokens']) {
    if (Object.hasOwn(advanced, forbidden)) {
      throw new Error(`Advanced Anthropic JSON must not include ${forbidden}`)
    }
  }
  for (const [key, fieldValue] of Object.entries(advanced)) {
    if (Object.hasOwn(body, key)) {
      throw new Error(`Advanced Anthropic JSON duplicates ${key}`)
    }
    body[key] = fieldValue
  }
}

function deepseekBaseUrlForProtocol(protocol: ModelProtocol): string {
  if (protocol === 'messages') {
    return 'https://api.deepseek.com/anthropic'
  }
  return 'https://api.deepseek.com'
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

function mergeAdvancedOpenAiResponsesOptions(options: OpenAiResponsesOptions, value: string) {
  const advanced = parseJsonObject(value, 'OpenAI advanced JSON') as OpenAiResponsesOptions
  for (const forbidden of [
    'model',
    'input',
    'stream',
    'tools',
    'max_output_tokens',
    'previous_response_id',
  ]) {
    if (Object.hasOwn(advanced, forbidden)) {
      throw new Error(`OpenAI advanced JSON must not include ${forbidden}`)
    }
  }
  for (const [key, fieldValue] of Object.entries(advanced) as Array<
    [keyof OpenAiResponsesOptions, OpenAiResponsesOptions[keyof OpenAiResponsesOptions]]
  >) {
    if (Object.hasOwn(options, key)) {
      throw new Error(`OpenAI advanced JSON duplicates ${String(key)}`)
    }
    options[key] = fieldValue as never
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function jsonText(value: unknown): string {
  if (value === undefined || value === null) {
    return ''
  }
  return JSON.stringify(value)
}

function parseJsonField(value: string): unknown | null {
  const trimmed = value.trim()
  if (!trimmed) {
    return null
  }
  try {
    return JSON.parse(trimmed)
  } catch {
    return null
  }
}

function invalidGeminiJsonField(values: ModelConfigFormValues): string | null {
  if (values.providerId !== 'gemini') {
    return null
  }
  for (const [field, label] of [
    ['responseJsonSchema', 'responseJsonSchema'],
    ['toolConfig', 'toolConfig'],
  ] as const) {
    if (!isValidJsonText(values[field])) {
      return label
    }
  }
  if (!isValidJsonText(values.safetySettings)) {
    return 'safetySettings'
  }
  const safetySettings = parseJsonField(values.safetySettings)
  if (safetySettings !== null && !Array.isArray(safetySettings)) {
    return 'safetySettings'
  }
  return null
}

function isValidJsonText(value: string): boolean {
  const trimmed = value.trim()
  if (!trimmed) {
    return true
  }
  try {
    JSON.parse(trimmed)
    return true
  } catch {
    return false
  }
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
  return (
    modelId === 'qwen3-max' ||
    modelId === 'qwen3-max-2026-01-23' ||
    modelId === 'qwen3.7-max' ||
    modelId === 'qwen3.7-max-preview' ||
    modelId.startsWith('qwen3.7-max-2026-')
  )
}

function isKimiWebSearchTool(value: unknown): boolean {
  if (!isRecord(value) || value.type !== 'builtin_function' || !isRecord(value.function)) {
    return false
  }
  return value.function.name === '$web_search'
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
