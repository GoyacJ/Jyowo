import { useEffect, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  useClearSkillSecret,
  useSetSkillConfigValue,
  useSetSkillSecret,
  useSkillConfig,
} from '@/features/skills/api/queries'
import type { SkillConfigDeclaration } from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Switch } from '@/shared/ui/switch'

type MutationTarget = `${'clear' | 'public' | 'secret'}:${string}`

export function SkillConfigPanel({ skillId }: { skillId: string }) {
  const { t } = useTranslation('skills')
  const configQuery = useSkillConfig(skillId)
  const setValueMutation = useSetSkillConfigValue()
  const setSecret = useSetSkillSecret()
  const clearSecretMutation = useClearSkillSecret()
  const [mutationTarget, setMutationTarget] = useState<MutationTarget | null>(null)
  const [mutationError, setMutationError] = useState<string | null>(null)

  useEffect(() => {
    setMutationError(null)
    setMutationTarget(null)
  }, [skillId])

  const savePublicValue = async (key: string, value: unknown) => {
    const target = `public:${key}` as const
    setMutationError(null)
    setMutationTarget(target)
    try {
      await setValueMutation.mutateAsync({ key, skillId, value })
      return true
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
      return false
    } finally {
      setMutationTarget(null)
    }
  }

  const saveSecret = async (key: string, value: string) => {
    const target = `secret:${key}` as const
    setMutationError(null)
    setMutationTarget(target)
    try {
      await setSecret({ key, skillId, value })
      return true
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
      return false
    } finally {
      setMutationTarget(null)
    }
  }

  const clearSecret = async (key: string) => {
    const target = `clear:${key}` as const
    setMutationError(null)
    setMutationTarget(target)
    try {
      await clearSecretMutation.mutateAsync({ key, skillId })
      return true
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
      return false
    } finally {
      setMutationTarget(null)
    }
  }

  if (configQuery.isLoading) {
    return <p className="text-muted-foreground text-sm">{t('config.loading')}</p>
  }

  if (configQuery.isError) {
    return (
      <div className="space-y-1">
        <p className="text-destructive text-sm">{t('config.loadError')}</p>
        <p className="break-words text-destructive text-xs" role="alert">
          {getCommandErrorMessage(configQuery.error)}
        </p>
      </div>
    )
  }

  const response = configQuery.data
  if (!response || response.declarations.length === 0) {
    return <p className="text-muted-foreground text-sm">{t('config.empty')}</p>
  }

  const mutationPending = mutationTarget !== null

  return (
    <section aria-label={t('config.title')} className="space-y-3">
      {response.declarations.map((declaration) => {
        if (declaration.secret) {
          return (
            <SecretConfigField
              configured={response.config.secrets[declaration.key]?.configured === true}
              declaration={declaration}
              disabled={mutationPending}
              key={declaration.key}
              onClear={() => clearSecret(declaration.key)}
              onSave={(value) => saveSecret(declaration.key, value)}
              pendingAction={
                mutationTarget === `clear:${declaration.key}`
                  ? 'clear'
                  : mutationTarget === `secret:${declaration.key}`
                    ? 'save'
                    : null
              }
              skillId={skillId}
            />
          )
        }

        return (
          <PublicConfigField
            declaration={declaration}
            disabled={mutationPending}
            key={declaration.key}
            onSave={(value) => savePublicValue(declaration.key, value)}
            pending={mutationTarget === `public:${declaration.key}`}
            skillId={skillId}
            value={response.config.values[declaration.key] ?? declaration.default}
          />
        )
      })}

      {mutationError ? (
        <p className="break-words text-destructive text-sm" role="alert">
          {mutationError}
        </p>
      ) : null}
    </section>
  )
}

function PublicConfigField({
  declaration,
  disabled,
  onSave,
  pending,
  skillId,
  value,
}: {
  declaration: SkillConfigDeclaration
  disabled: boolean
  onSave: (value: unknown) => Promise<boolean>
  pending: boolean
  skillId: string
  value: unknown
}) {
  const { t } = useTranslation('skills')
  const generatedId = useId()
  const inputId = `skill-config-${generatedId}`
  const descriptionId = declaration.description ? `${inputId}-description` : undefined
  const [draft, setDraft] = useState(() => publicDraftValue(declaration, value))

  useEffect(() => {
    setDraft(publicDraftValue(declaration, value))
  }, [declaration, skillId, value])

  const submit = async () => {
    await onSave(publicMutationValue(declaration, draft))
  }

  return (
    <div className="rounded-md border border-border bg-surface p-3">
      <ConfigFieldHeader declaration={declaration} inputId={inputId} />
      {declaration.description ? (
        <p className="mt-1 text-muted-foreground text-xs" id={descriptionId}>
          {declaration.description}
        </p>
      ) : null}

      <div className="mt-3 flex items-center gap-2">
        {declaration.valueType === 'boolean' ? (
          <Switch
            aria-describedby={descriptionId}
            aria-label={declaration.key}
            checked={draft === true}
            disabled={disabled}
            id={inputId}
            onCheckedChange={setDraft}
          />
        ) : (
          <Input
            aria-describedby={descriptionId}
            disabled={disabled}
            id={inputId}
            onChange={(event) => setDraft(event.target.value)}
            required={declaration.required}
            type={publicInputType(declaration)}
            value={typeof draft === 'string' ? draft : ''}
          />
        )}
        <Button disabled={disabled} onClick={() => void submit()} size="sm" type="button">
          {pending ? `${t('config.save')}…` : t('config.save')}
        </Button>
      </div>
    </div>
  )
}

function SecretConfigField({
  configured,
  declaration,
  disabled,
  onClear,
  onSave,
  pendingAction,
  skillId,
}: {
  configured: boolean
  declaration: SkillConfigDeclaration
  disabled: boolean
  onClear: () => Promise<boolean>
  onSave: (value: string) => Promise<boolean>
  pendingAction: 'clear' | 'save' | null
  skillId: string
}) {
  const { t } = useTranslation('skills')
  const generatedId = useId()
  const inputId = `skill-config-${generatedId}`
  const descriptionId = declaration.description ? `${inputId}-description` : undefined
  const [draft, setDraft] = useState('')

  useEffect(() => {
    setDraft('')
  }, [skillId])

  const save = async () => {
    if (!draft) return
    if (await onSave(draft)) setDraft('')
  }

  const clear = async () => {
    if (await onClear()) setDraft('')
  }

  return (
    <div className="rounded-md border border-border bg-surface p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <ConfigFieldHeader declaration={declaration} inputId={inputId} />
        <Badge variant={configured ? 'success' : 'outline'}>
          {configured ? t('config.configured') : t('config.notConfigured')}
        </Badge>
      </div>
      {declaration.description ? (
        <p className="mt-1 text-muted-foreground text-xs" id={descriptionId}>
          {declaration.description}
        </p>
      ) : null}

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Input
          aria-describedby={descriptionId}
          autoComplete="new-password"
          className="min-w-52 flex-1"
          disabled={disabled}
          id={inputId}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={t('config.secretPlaceholder')}
          type="password"
          value={draft}
        />
        <Button
          disabled={disabled || draft.length === 0}
          onClick={() => void save()}
          size="sm"
          type="button"
        >
          {pendingAction === 'save'
            ? `${configured ? t('config.replace') : t('config.set')}…`
            : configured
              ? t('config.replace')
              : t('config.set')}
        </Button>
        {configured ? (
          <Button
            disabled={disabled}
            onClick={() => void clear()}
            size="sm"
            type="button"
            variant="outline"
          >
            {pendingAction === 'clear' ? `${t('config.clear')}…` : t('config.clear')}
          </Button>
        ) : null}
      </div>
    </div>
  )
}

function ConfigFieldHeader({
  declaration,
  inputId,
}: {
  declaration: SkillConfigDeclaration
  inputId: string
}) {
  const { t } = useTranslation('skills')

  return (
    <div className="flex flex-wrap items-center gap-2">
      <label className="font-medium font-mono text-sm" htmlFor={inputId}>
        {declaration.key}
      </label>
      <Badge variant="outline">
        {declaration.required ? t('config.required') : t('config.optional')}
      </Badge>
    </div>
  )
}

function publicDraftValue(declaration: SkillConfigDeclaration, value: unknown): string | boolean {
  if (declaration.valueType === 'boolean') return value === true
  if (value === undefined || value === null) return ''
  return String(value)
}

function publicMutationValue(
  declaration: SkillConfigDeclaration,
  draft: string | boolean,
): string | number | boolean {
  if (declaration.valueType === 'boolean') return draft === true
  if (declaration.valueType === 'number') return Number(draft)
  return typeof draft === 'string' ? draft : String(draft)
}

function publicInputType(declaration: SkillConfigDeclaration) {
  if (declaration.valueType === 'number') return 'number'
  if (declaration.valueType === 'url') return 'url'
  return 'text'
}
