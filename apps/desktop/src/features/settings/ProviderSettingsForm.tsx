import { KeyRound, Save } from 'lucide-react'
import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import type { SaveProviderSettingsResponse } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

const providerSettingsFormSchema = z
  .object({
    apiKey: z.string().trim().min(1, 'API key is required.'),
    modelId: z.string().trim().min(1, 'Model is required.'),
    providerId: z.enum([
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
    ]),
  })
  .strict()

type ProviderSettingsFormValues = z.infer<typeof providerSettingsFormSchema>

export function ProviderSettingsForm() {
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
      setFormError('Provider settings could not be saved.')
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
          <h2 className="font-semibold text-base">Provider settings</h2>
          <p className="mt-1 text-muted-foreground text-sm">
            Configure the model endpoint used by local runs.
          </p>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <label className="space-y-2 text-sm">
          <span className="font-medium">Provider</span>
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
          <span className="font-medium">Model</span>
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
        <span className="font-medium">API key</span>
        <input
          className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          disabled={isSubmitting}
          placeholder="Stored after save"
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
          <div className="font-medium">Provider saved.</div>
          <div className="mt-1 text-muted-foreground">Stored as secret reference</div>
          <div className="mt-1 break-all font-mono text-xs">{savedSettings.secretRef}</div>
        </div>
      ) : null}

      <div className="flex justify-end">
        <Button disabled={isSubmitting} type="submit">
          <Save className="size-4" />
          {isSubmitting ? 'Saving provider settings' : 'Save provider settings'}
        </Button>
      </div>
    </form>
  )
}
