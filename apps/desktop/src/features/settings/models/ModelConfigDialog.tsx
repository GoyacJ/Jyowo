import { useEffect, useMemo, useRef } from 'react'
import { useForm } from 'react-hook-form'
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

type ModelConfigDialogProps = {
  catalog: ModelProviderCatalogResponse
  open: boolean
  profile?: ProviderConfig | null
  onOpenChange: (open: boolean) => void
  onSaved?: (config: ProviderConfig) => void
}

type ModelProtocol = NonNullable<ProviderSettingsRequest['protocol']>

type ModelConfigFormValues = {
  baseUrl: string
  codeInterpreter: boolean
  displayName: string
  enableThinking: boolean
  modelId: string
  protocol: ModelProtocol
  providerId: string
  reasoningEffort: string
  sessionCache: boolean
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
    if (displayName) {
      request.displayName = displayName
    }
    if (baseUrl) {
      request.baseUrl = baseUrl
    }
    if (values.providerId === 'qwen') {
      request.protocol = values.protocol
      request.providerDefaults = providerDefaultsFromValues(values)
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
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.profileName')}</span>
            <input
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              {...register('displayName')}
            />
          </label>

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.provider')}</span>
            <select
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              {...register('providerId', {
                required: t('provider.errors.providerRequired'),
                onChange: (event) => {
                  const provider = providers.find((candidate) => candidate.providerId === event.target.value)
                  setValue('baseUrl', provider?.defaultBaseUrl ?? '')
                  setValue('modelId', provider?.models[0]?.modelId ?? '')
                  setValue('protocol', defaultProtocolForProvider(provider))
                  setValue('enableThinking', false)
                  setValue('reasoningEffort', '')
                  setValue('webSearch', false)
                  setValue('codeInterpreter', false)
                  setValue('webExtractor', false)
                  setValue('sessionCache', false)
                },
              })}
            >
              {providers.map((provider) => (
                <option key={provider.providerId} value={provider.providerId}>
                  {provider.displayName}
                </option>
              ))}
            </select>
          </label>

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.model')}</span>
            <select
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              {...register('modelId', { required: t('provider.errors.modelRequired') })}
            >
              {modelOptions.map((model) => (
                <option key={model.modelId} value={model.modelId}>
                  {model.displayName}
                </option>
              ))}
            </select>
          </label>

          {isQwen ? (
            <div className="grid gap-3 rounded-sm border border-border p-3 text-sm">
              <label className="grid gap-1">
                <span className="font-medium">{t('provider.apiMode')}</span>
                <select
                  className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  {...register('protocol')}
                >
                  <option value="responses">Responses</option>
                  <option value="chat_completions">Chat Completions</option>
                </select>
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" {...register('enableThinking')} />
                <span>{t('provider.enableThinking')}</span>
              </label>
              <label className="grid gap-1">
                <span className="font-medium">{t('provider.reasoningEffort')}</span>
                <select
                  className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  {...register('reasoningEffort')}
                >
                  <option value="">{t('provider.default')}</option>
                  <option value="none">None</option>
                  <option value="minimal">Minimal</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                </select>
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

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.baseUrl')}</span>
            <input
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              placeholder={selectedProvider?.defaultBaseUrl}
              {...register('baseUrl')}
            />
          </label>

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.apiKey')}</span>
            <input
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              placeholder={
                profile?.hasApiKey
                  ? t('provider.apiKeyExistingPlaceholder')
                  : t('provider.apiKeyPlaceholder')
              }
              type="password"
              name="apiKey"
            />
          </label>

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.officialQuotaApiKey')}</span>
            <input
              className="h-9 rounded-sm border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
  return {
    baseUrl: profile?.baseUrl ?? defaultProvider?.defaultBaseUrl ?? '',
    codeInterpreter: defaults.codeInterpreter,
    displayName: profile?.displayName ?? '',
    enableThinking: defaults.enableThinking,
    modelId: profile?.modelId ?? defaultModel?.modelId ?? '',
    protocol: profile?.protocol ?? defaultProtocolForProvider(defaultProvider),
    providerId: profile?.providerId ?? defaultProvider?.providerId ?? '',
    reasoningEffort: defaults.reasoningEffort,
    sessionCache: defaults.sessionCache,
    webExtractor: defaults.webExtractor,
    webSearch: defaults.webSearch,
  }
}

function defaultProtocolForProvider(
  provider: ModelProviderCatalogResponse['providers'][number] | undefined,
): ModelProtocol {
  return (provider?.providerId === 'qwen' ? 'responses' : provider?.models[0]?.protocol) ?? 'responses'
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

function providerDefaultsFromValues(values: ModelConfigFormValues): ProviderSettingsRequest['providerDefaults'] {
  const body: Record<string, unknown> = {}
  const headers: Record<string, string> = {}
  const tools: Array<{ type: string }> = []

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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
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
