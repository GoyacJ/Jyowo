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
type OpenAiResponsesOptions = NonNullable<
  NonNullable<ProviderSettingsRequest['modelOptions']>['openaiResponses']
>

type ModelConfigFormValues = {
  baseUrl: string
  codeInterpreter: boolean
  displayName: string
  enableThinking: boolean
  thinkingType: string
  thinkingDisplay: string
  cacheTtl: string
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
  const isOpenAI = selectedProvider?.providerId === 'openai'
  const isAnthropic = selectedProvider?.providerId === 'anthropic'
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
  const providerCapabilityMetadata = getAnthropicCapabilityMetadata(
    selectedModel?.providerCapabilityMetadata,
  )
  const anthropicSamplingLocked = providerCapabilityMetadata?.samplingLocked === true
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
    if (displayName) {
      request.displayName = displayName
    }
    if (baseUrl) {
      request.baseUrl = baseUrl
    }
    if (values.providerId === 'qwen') {
      request.protocol = values.protocol
      request.providerDefaults = providerDefaultsFromValues(values)
    } else if (values.providerId === 'openai') {
      // OpenAI Responses request fields are typed model options, not provider defaults.
    } else {
      let providerDefaults: ProviderSettingsRequest['providerDefaults']
      try {
        providerDefaults = providerDefaultsFromValues(values)
      } catch (error) {
        setError('root', {
          message: error instanceof Error ? error.message : 'Invalid provider defaults',
        })
        return
      }
      if (hasProviderDefaults(providerDefaults)) {
        request.providerDefaults = providerDefaults
      }
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
      <DialogContent className="max-h-[calc(100vh-2rem)] w-[min(calc(100vw-2rem),36rem)] overflow-y-auto">
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
                  <Select id="provider-openai-text-verbosity" {...register('openaiTextVerbosity')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="low">low</option>
                    <option value="medium">medium</option>
                    <option value="high">high</option>
                  </Select>
                </label>
                <label className="grid gap-1" htmlFor="provider-openai-service-tier">
                  <span className="font-medium">OpenAI service tier</span>
                  <Select id="provider-openai-service-tier" {...register('openaiServiceTier')}>
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
                <label className="grid gap-1" htmlFor="provider-openai-prompt-cache-retention">
                  <span className="font-medium">OpenAI prompt cache retention</span>
                  <Input
                    id="provider-openai-prompt-cache-retention"
                    {...register('openaiPromptCacheRetention')}
                  />
                </label>
                <label className="grid gap-1" htmlFor="provider-openai-instructions">
                  <span className="font-medium">OpenAI instructions</span>
                  <Input id="provider-openai-instructions" {...register('openaiInstructions')} />
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
                  <Input id="provider-openai-metadata-json" {...register('openaiMetadataJson')} />
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
                  <Input id="provider-openai-advanced-json" {...register('openaiAdvancedJson')} />
                </label>
              </div>
            </div>
          ) : null}

          {!isQwen && supportedParameters.size > 0 ? (
            <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
              <span className="font-medium">{t('provider.providerOptions')}</span>
              {supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) ? (
                <label className="flex items-center gap-2">
                  <input type="checkbox" {...register('enableThinking')} />
                  <span>{t('provider.enableThinking')}</span>
                </label>
              ) : null}
              {supportsAny(supportedParameters, ['thinking', 'thinkingConfig']) ? (
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
              {supportedParameters.has('service_tier') ? (
                <label className="grid gap-1" htmlFor="provider-service-tier">
                  <span className="font-medium">{t('provider.serviceTier')}</span>
                  <Select id="provider-service-tier" {...register('serviceTier')}>
                    <option value="">{t('provider.default')}</option>
                    <option value="auto">Auto</option>
                    <option value="standard_only">Standard only</option>
                  </Select>
                </label>
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
              {supportsAny(supportedParameters, ['stop_sequences', 'stopSequences']) ? (
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
                    <Input id="provider-user-profile-id" {...register('anthropicUserProfileId')} />
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
  const openaiDefaults = openAiResponsesDefaultsFromProfile(profile)
  return {
    baseUrl: profile?.baseUrl ?? defaultProvider?.defaultBaseUrl ?? '',
    anthropicAdvancedJson: providerDefaults.anthropicAdvancedJson,
    anthropicBeta: providerDefaults.anthropicBeta,
    anthropicUserProfileId: providerDefaults.anthropicUserProfileId,
    cacheTtl: providerDefaults.cacheTtl,
    codeInterpreter: defaults.codeInterpreter,
    contextManagementJson: providerDefaults.contextManagementJson,
    displayName: profile?.displayName ?? '',
    enableThinking: defaults.enableThinking || providerDefaults.enableThinking,
    fallbacksJson: providerDefaults.fallbacksJson,
    inferenceGeo: providerDefaults.inferenceGeo,
    metadataJson: providerDefaults.metadataJson,
    modelId: profile?.modelId ?? defaultModel?.modelId ?? '',
    openaiAdvancedJson: '',
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
    protocol: profile?.protocol ?? defaultProtocolForProvider(defaultProvider),
    providerId: profile?.providerId ?? defaultProvider?.providerId ?? '',
    reasoningEffort: defaults.reasoningEffort,
    responseMimeType: providerDefaults.responseMimeType,
    seed: providerDefaults.seed,
    serviceTier: providerDefaults.serviceTier,
    sessionCache: defaults.sessionCache,
    stopSequences: providerDefaults.stopSequences,
    thinkingBudget: providerDefaults.thinkingBudget,
    thinkingDisplay: providerDefaults.thinkingDisplay,
    thinkingType: providerDefaults.thinkingType,
    topK: providerDefaults.topK,
    topP: providerDefaults.topP,
    toolChoice: providerDefaults.toolChoice,
    toolName: providerDefaults.toolName,
    speed: providerDefaults.speed,
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
  const cacheControl = isRecord(body.cache_control) ? body.cache_control : null
  const toolChoice = isRecord(body.tool_choice) ? body.tool_choice : null
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
    responseMimeType: typeof body.responseMimeType === 'string' ? body.responseMimeType : '',
    seed: firstStringable(body.seed),
    serviceTier: typeof body.service_tier === 'string' ? body.service_tier : '',
    stopSequences: stopSequences.join(','),
    thinkingBudget: firstStringable(thinking?.budget_tokens, thinkingConfig?.thinkingBudget),
    thinkingDisplay: typeof thinking?.display === 'string' ? thinking.display : '',
    thinkingType: typeof thinking?.type === 'string' ? thinking.type : '',
    topK,
    topP,
    toolChoice: typeof toolChoice?.type === 'string' ? toolChoice.type : '',
    toolName: typeof toolChoice?.name === 'string' ? toolChoice.name : '',
    speed: typeof body.speed === 'string' ? body.speed : '',
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

function providerDefaultsFromValues(
  values: ModelConfigFormValues,
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
      return { body, headers }
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
    if (values.enableThinking) {
      body.thinking = { type: 'enabled' }
    }
    return { body, headers }
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

  return { body, headers }
}

function hasProviderDefaults(defaults: ProviderSettingsRequest['providerDefaults']): boolean {
  return (
    Object.keys(defaults?.body ?? {}).length > 0 || Object.keys(defaults?.headers ?? {}).length > 0
  )
}

function resetProviderOptionFields(setValue: UseFormSetValue<ModelConfigFormValues>) {
  setValue('enableThinking', false)
  setValue('thinkingType', '')
  setValue('thinkingDisplay', '')
  setValue('cacheTtl', '')
  setValue('outputEffort', '')
  setValue('performanceLatency', '')
  setValue('reasoningEffort', '')
  setValue('responseMimeType', '')
  setValue('seed', '')
  setValue('serviceTier', '')
  setValue('stopSequences', '')
  setValue('thinkingBudget', '')
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
  thinkingModes?: string[]
  samplingLocked?: boolean
}

function getAnthropicCapabilityMetadata(value: unknown): AnthropicCapabilityMetadata | null {
  if (!isRecord(value) || value.provider !== 'anthropic') {
    return null
  }
  return {
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
