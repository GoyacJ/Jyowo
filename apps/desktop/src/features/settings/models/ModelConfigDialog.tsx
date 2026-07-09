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

type ModelConfigFormValues = {
  baseUrl: string
  displayName: string
  modelId: string
  modelOptionsJson: string
  providerId: string
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
    const modelOptions = parseModelOptionsJson(values.modelOptionsJson)

    if (!modelOptions.ok) {
      setError('modelOptionsJson', { message: t('provider.errors.modelOptionsInvalid') })
      return
    }

    if (profile) {
      request.configId = profile.id
      request.setDefault = profile.isDefault
    }
    request.modelOptions = modelOptions.value
    if (displayName) {
      request.displayName = displayName
    }
    if (baseUrl) {
      request.baseUrl = baseUrl
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
              {...register('providerId', { required: t('provider.errors.providerRequired') })}
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

          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t('provider.modelOptions')}</span>
            <textarea
              className="min-h-28 rounded-sm border border-input bg-background px-2 py-2 font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
              spellCheck={false}
              {...register('modelOptionsJson')}
            />
          </label>

          {errors.root?.message ? (
            <p className="text-destructive text-sm" role="alert">
              {errors.root.message}
            </p>
          ) : null}
          {errors.modelOptionsJson?.message ? (
            <p className="text-destructive text-sm" role="alert">
              {errors.modelOptionsJson.message}
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
  return {
    baseUrl: profile?.baseUrl ?? defaultProvider?.defaultBaseUrl ?? '',
    displayName: profile?.displayName ?? '',
    modelId: profile?.modelId ?? defaultModel?.modelId ?? '',
    modelOptionsJson: formatModelOptionsJson(profile?.modelOptions),
    providerId: profile?.providerId ?? defaultProvider?.providerId ?? '',
  }
}

function formatModelOptionsJson(
  modelOptions: ProviderSettingsRequest['modelOptions'] | undefined,
): string {
  return JSON.stringify(modelOptions ?? {}, null, 2)
}

function parseModelOptionsJson(
  value: string,
): { ok: true; value: ProviderSettingsRequest['modelOptions'] } | { ok: false } {
  try {
    const parsed = value.trim() ? JSON.parse(value) : {}
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== 'object') {
      return { ok: false }
    }
    return { ok: true, value: parsed as ProviderSettingsRequest['modelOptions'] }
  } catch {
    return { ok: false }
  }
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
