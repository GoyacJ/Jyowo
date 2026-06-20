import { KeyRound, Save } from 'lucide-react'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'

import type { SaveProviderSettingsResponse } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

const providerIds = [
  'anthropic',
  'codex',
  'deepseek',
  'doubao',
  'gemini',
  'local-llama',
  'minimax',
  'openai',
  'openrouter',
  'qwen',
  'zhipu',
] as const

type ProviderSettingsFormValues = {
  apiKey: string
  modelId: string
  providerId: (typeof providerIds)[number]
}

export function ProviderSettingsForm() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const [formError, setFormError] = useState<string | null>(null)
  const [savedSettings, setSavedSettings] = useState<SaveProviderSettingsResponse | null>(null)
  const {
    formState: { errors, isSubmitting },
    handleSubmit,
    register,
    setError,
    setValue,
  } = useForm<ProviderSettingsFormValues>({
    defaultValues: {
      apiKey: '',
      modelId: '',
      providerId: 'openai',
    },
  })
  const providerSettingsFormSchema = z
    .object({
      apiKey: z.string().trim().min(1, t('provider.errors.apiKeyRequired')),
      modelId: z.string().trim().min(1, t('provider.errors.modelRequired')),
      providerId: z.enum(providerIds),
    })
    .strict()

  async function submit(values: ProviderSettingsFormValues) {
    setFormError(null)
    setSavedSettings(null)

    const parsed = providerSettingsFormSchema.safeParse(values)
    if (!parsed.success) {
      for (const issue of parsed.error.issues) {
        const field = issue.path[0]
        if (field === 'apiKey' || field === 'modelId' || field === 'providerId') {
          setError(field, { message: issue.message, type: 'manual' })
        }
      }
      return
    }

    const request = parsed.data
    setValue('apiKey', '')

    try {
      const saved = await commandClient.saveProviderSettings(request)
      setSavedSettings(saved)
    } catch {
      setFormError(t('provider.saveError'))
    }
  }

  return (
    <form
      className="space-y-5 rounded-md border border-border bg-surface p-5"
      onSubmit={handleSubmit(submit)}
    >
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <KeyRound className="size-4" />
        </div>
        <div>
          <h2 className="font-semibold text-base">{t('provider.title')}</h2>
          <p className="mt-1 text-muted-foreground text-sm">{t('provider.description')}</p>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <label className="space-y-2 text-sm">
          <span className="font-medium">{t('provider.provider')}</span>
          <select
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            {...register('providerId')}
          >
            <option value="openai">OpenAI</option>
            <option value="local-llama">Local Llama</option>
            <option value="anthropic">Anthropic</option>
            <option value="openrouter">OpenRouter</option>
            <option value="codex">Codex</option>
            <option value="deepseek">DeepSeek</option>
            <option value="qwen">Qwen</option>
            <option value="gemini">Gemini</option>
            <option value="doubao">Doubao</option>
            <option value="zhipu">Zhipu</option>
            <option value="minimax">Minimax</option>
          </select>
        </label>

        <label className="space-y-2 text-sm">
          <span className="font-medium">{t('provider.model')}</span>
          <input
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            placeholder="gpt-4o-mini"
            {...register('modelId')}
          />
          {errors.modelId ? (
            <span className="block text-destructive text-xs">{errors.modelId.message}</span>
          ) : null}
        </label>
      </div>

      <label className="block space-y-2 text-sm">
        <span className="font-medium">{t('provider.apiKey')}</span>
        <input
          className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          disabled={isSubmitting}
          placeholder={t('provider.apiKeyPlaceholder')}
          type="password"
          {...register('apiKey')}
        />
        {errors.apiKey ? (
          <span className="block text-destructive text-xs">{errors.apiKey.message}</span>
        ) : null}
      </label>

      {formError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          {formError}
        </div>
      ) : null}

      {savedSettings ? (
        <div className="rounded-md border border-border bg-background px-3 py-2 text-sm">
          <div className="font-medium">{t('provider.saved')}</div>
          <div className="mt-1 text-muted-foreground">{t('provider.secretReference')}</div>
          <div className="mt-1 break-all font-mono text-xs">{savedSettings.secretRef}</div>
        </div>
      ) : null}

      <div className="flex justify-end">
        <Button disabled={isSubmitting} type="submit">
          <Save className="size-4" />
          {isSubmitting ? t('provider.saving') : t('provider.save')}
        </Button>
      </div>
    </form>
  )
}
