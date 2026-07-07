import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlarmClock, Play, Save, Trash2 } from 'lucide-react'
import { type FormEvent, type ReactNode, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useActiveProjectPath } from '@/features/workspace/use-active-project-path'
import type {
  AutomationRunRecord,
  AutomationSpec,
  PermissionMode,
  ToolProfile,
} from '@/shared/tauri/commands'
import {
  deleteAutomation,
  hasObviousUnredactedSecret,
  listAutomationRuns,
  listAutomations,
  runAutomationNow,
  saveAutomation,
  setAutomationEnabled,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Switch } from '@/shared/ui/switch'

const automationQueryKeys = {
  all: ['automations'] as const,
  list: () => [...automationQueryKeys.all, 'list'] as const,
}

const automationRunQueryKeys = {
  all: ['automation-runs'] as const,
  list: () => [...automationRunQueryKeys.all, 'list'] as const,
}

const toolProfileOptions = ['minimal', 'coding', 'full'] as const
type PresetToolProfile = Extract<ToolProfile, (typeof toolProfileOptions)[number]>

const permissionModeOptions = ['default', 'auto', 'bypass_permissions'] as const
const missedRunPolicyOptions = ['skip', 'run_once'] as const
const automationIdPattern = /^[A-Za-z0-9][A-Za-z0-9._-]*$/

export function AutomationSettings() {
  const { t } = useTranslation('settings')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const activeProjectPathQuery = useActiveProjectPath()
  const hasProjectScope = activeProjectPathQuery.data != null
  const formRef = useRef<HTMLFormElement>(null)
  const [formError, setFormError] = useState<string | null>(null)
  const [operationError, setOperationError] = useState<string | null>(null)
  const automationsQuery = useQuery({
    queryKey: automationQueryKeys.list(),
    queryFn: () => listAutomations(commandClient),
  })
  const runsQuery = useQuery({
    queryKey: automationRunQueryKeys.list(),
    queryFn: () => listAutomationRuns(undefined, commandClient),
  })
  const saveMutation = useMutation({
    mutationFn: (automation: AutomationSpec) => saveAutomation({ automation }, commandClient),
    onSuccess: async () => {
      formRef.current?.reset()
      setFormError(null)
      await queryClient.invalidateQueries({ queryKey: automationQueryKeys.all })
    },
  })
  const toggleMutation = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      setAutomationEnabled(id, enabled, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: automationQueryKeys.all })
    },
  })
  const runNowMutation = useMutation({
    mutationFn: (id: string) => runAutomationNow(id, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: automationRunQueryKeys.all })
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAutomation(id, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: automationQueryKeys.all })
    },
  })
  const automations = automationsQuery.data?.automations ?? []
  const runs = runsQuery.data?.runs ?? []

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setFormError(null)
    setOperationError(null)

    const formData = new FormData(event.currentTarget)
    const id = String(formData.get('id') ?? '').trim()
    const prompt = String(formData.get('prompt') ?? '').trim()
    const intervalMinutes = Number(formData.get('intervalMinutes') ?? 0)
    const toolProfile = String(formData.get('toolProfile') ?? 'coding') as PresetToolProfile
    const permissionMode = String(formData.get('permissionMode') ?? 'default') as PermissionMode
    const missedRunPolicy = String(
      formData.get('missedRunPolicy') ?? 'skip',
    ) as AutomationSpec['missedRunPolicy']
    const enabled = formData.get('enabled') === 'on'

    if (!automationIdPattern.test(id)) {
      setFormError(t('automation.errors.invalidId'))
      return
    }
    if (prompt.length === 0) {
      setFormError(t('automation.errors.promptRequired'))
      return
    }
    if (hasObviousUnredactedSecret(prompt)) {
      setFormError(t('automation.errors.secretPrompt'))
      return
    }
    if (!Number.isInteger(intervalMinutes) || intervalMinutes <= 0) {
      setFormError(t('automation.errors.intervalRequired'))
      return
    }

    const existing = automations.find((automation) => automation.id === id)
    const now = new Date().toISOString()

    try {
      await saveMutation.mutateAsync({
        createdAt: existing?.createdAt ?? now,
        enabled,
        id,
        missedRunPolicy,
        permissionMode,
        prompt,
        sandboxMode: 'none',
        schedule: { intervalMinutes },
        toolProfile,
        updatedAt: now,
        workspaceAccess: 'read_only',
        workspaceScope: 'current_workspace',
      })
    } catch {
      setOperationError(t('automation.errors.save'))
    }
  }

  async function toggleAutomation(automation: AutomationSpec) {
    setOperationError(null)
    try {
      await toggleMutation.mutateAsync({ enabled: !automation.enabled, id: automation.id })
    } catch {
      setOperationError(t('automation.errors.operation'))
    }
  }

  async function runAutomation(automation: AutomationSpec) {
    setOperationError(null)
    try {
      await runNowMutation.mutateAsync(automation.id)
    } catch {
      setOperationError(t('automation.errors.operation'))
    }
  }

  async function removeAutomation(automation: AutomationSpec) {
    setOperationError(null)
    try {
      await deleteMutation.mutateAsync(automation.id)
    } catch {
      setOperationError(t('automation.errors.operation'))
    }
  }

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <AlarmClock className="size-4" />
        </div>
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="font-semibold text-base">{t('automation.title')}</h2>
            <Badge variant="outline">
              {hasProjectScope ? t('scope.projectOverrides') : t('scope.runtimeDiagnostics')}
            </Badge>
          </div>
          <p className="mt-1 text-muted-foreground text-sm">{t('automation.description')}</p>
        </div>
      </div>

      {automationsQuery.isLoading ? (
        <p className="text-muted-foreground text-sm">{t('automation.loading')}</p>
      ) : null}
      {automationsQuery.isError ? (
        <p className="text-destructive text-sm">{t('automation.loadError')}</p>
      ) : null}

      {!automationsQuery.isLoading && !automationsQuery.isError ? (
        <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
          <div className="space-y-4">
            {operationError ? <p className="text-destructive text-sm">{operationError}</p> : null}
            {automations.length === 0 ? (
              <div className="rounded-md border border-dashed border-border bg-background p-4 text-muted-foreground text-sm">
                {t('automation.empty')}
              </div>
            ) : (
              <div className="space-y-3">
                {automations.map((automation) => (
                  <AutomationCard
                    automation={automation}
                    key={automation.id}
                    onDelete={() => void removeAutomation(automation)}
                    onRun={() => void runAutomation(automation)}
                    onToggle={() => void toggleAutomation(automation)}
                  />
                ))}
              </div>
            )}

            {hasProjectScope ? (
              <div className="space-y-3 rounded-md border border-border bg-background p-4">
                <h3 className="font-medium text-sm">{t('automation.form.title')}</h3>
                <form className="space-y-4" onSubmit={submit} ref={formRef}>
                  <div className="grid gap-3 md:grid-cols-2">
                    <Field fieldId="automation-id" label={t('automation.form.id')}>
                      <input
                        className={inputClassName}
                        id="automation-id"
                        name="id"
                        placeholder="nightly-checks"
                      />
                    </Field>
                    <Field fieldId="automation-interval" label={t('automation.form.interval')}>
                      <input
                        className={inputClassName}
                        defaultValue={30}
                        id="automation-interval"
                        min={1}
                        name="intervalMinutes"
                        type="number"
                      />
                    </Field>
                    <Field
                      fieldId="automation-tool-profile"
                      label={t('automation.form.toolProfile')}
                    >
                      <select
                        className={inputClassName}
                        defaultValue="coding"
                        id="automation-tool-profile"
                        name="toolProfile"
                      >
                        {toolProfileOptions.map((toolProfile) => (
                          <option key={toolProfile} value={toolProfile}>
                            {t(`automation.toolProfile.${toolProfile}`)}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field
                      fieldId="automation-permission-mode"
                      label={t('automation.form.permissionMode')}
                    >
                      <select
                        className={inputClassName}
                        defaultValue="default"
                        id="automation-permission-mode"
                        name="permissionMode"
                      >
                        {permissionModeOptions.map((permissionMode) => (
                          <option key={permissionMode} value={permissionMode}>
                            {t(`automation.permissionMode.${permissionMode}`)}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field
                      fieldId="automation-missed-policy"
                      label={t('automation.form.missedRunPolicy')}
                    >
                      <select
                        className={inputClassName}
                        defaultValue="skip"
                        id="automation-missed-policy"
                        name="missedRunPolicy"
                      >
                        {missedRunPolicyOptions.map((policy) => (
                          <option key={policy} value={policy}>
                            {t(`automation.missedRunPolicy.${policy}`)}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <label className="flex items-center gap-2 self-end text-sm">
                      <input className="size-4" name="enabled" type="checkbox" />
                      {t('automation.form.enabled')}
                    </label>
                    <div className="md:col-span-2">
                      <Field fieldId="automation-prompt" label={t('automation.form.prompt')}>
                        <textarea
                          className={`${inputClassName} min-h-24 resize-y py-2`}
                          id="automation-prompt"
                          name="prompt"
                          placeholder={t('automation.form.promptPlaceholder')}
                        />
                      </Field>
                    </div>
                  </div>
                  {formError ? <p className="text-destructive text-sm">{formError}</p> : null}
                  <div className="flex justify-end">
                    <Button disabled={saveMutation.isPending} type="submit">
                      <Save className="size-4" />
                      {saveMutation.isPending ? t('automation.saving') : t('automation.save')}
                    </Button>
                  </div>
                </form>
              </div>
            ) : null}
          </div>

          <aside className="space-y-3 rounded-md border border-border bg-background p-4">
            <h3 className="font-medium text-sm">{t('automation.runs.title')}</h3>
            {runsQuery.isLoading ? (
              <p className="text-muted-foreground text-sm">{t('automation.runs.loading')}</p>
            ) : null}
            {runsQuery.isError ? (
              <p className="text-destructive text-sm">{t('automation.runs.loadError')}</p>
            ) : null}
            {!runsQuery.isLoading && !runsQuery.isError && runs.length === 0 ? (
              <p className="text-muted-foreground text-sm">{t('automation.runs.empty')}</p>
            ) : null}
            {!runsQuery.isLoading && !runsQuery.isError && runs.length > 0 ? (
              <div className="space-y-3">
                {runs.slice(0, 8).map((run) => (
                  <div className="space-y-1 rounded-sm border border-border p-3" key={run.id}>
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate font-medium text-sm">{run.automationId}</span>
                      <Badge variant={runStatusBadgeVariant(run.status)}>{run.status}</Badge>
                    </div>
                    <p className="text-muted-foreground text-xs">{formatDate(run.startedAt)}</p>
                  </div>
                ))}
              </div>
            ) : null}
          </aside>
        </div>
      ) : null}
    </section>
  )
}

function AutomationCard({
  automation,
  onDelete,
  onRun,
  onToggle,
}: {
  automation: AutomationSpec
  onDelete: () => void
  onRun: () => void
  onToggle: () => void
}) {
  const { t } = useTranslation('settings')
  const promptPreview = hasObviousUnredactedSecret(automation.prompt)
    ? t('automation.promptWithheld')
    : automation.prompt

  return (
    <article
      aria-label={automation.id}
      className="space-y-3 rounded-md border border-border bg-background p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate font-semibold text-sm">{automation.id}</h3>
            <Badge variant={automation.enabled ? 'success' : 'outline'}>
              {automation.enabled ? t('automation.enabled') : t('automation.disabled')}
            </Badge>
          </div>
          <p className="line-clamp-2 text-muted-foreground text-sm">{promptPreview}</p>
        </div>
        <Switch
          aria-label={t(automation.enabled ? 'automation.disableNamed' : 'automation.enableNamed', {
            id: automation.id,
          })}
          checked={automation.enabled}
          onCheckedChange={onToggle}
        />
      </div>
      <div className="grid gap-2 text-sm sm:grid-cols-2">
        <Detail label={t('automation.form.interval')}>
          {t('automation.intervalMinutes', { count: automation.schedule.intervalMinutes })}
        </Detail>
        <Detail label={t('automation.form.toolProfile')}>
          {simpleToolProfileLabel(automation.toolProfile, t)}
        </Detail>
        <Detail label={t('automation.form.permissionMode')}>
          {t(`automation.permissionMode.${automation.permissionMode}`)}
        </Detail>
        <Detail label={t('automation.form.missedRunPolicy')}>
          {t(`automation.missedRunPolicy.${automation.missedRunPolicy}`)}
        </Detail>
      </div>
      <div className="flex flex-wrap justify-end gap-2">
        <Button
          aria-label={t('automation.runNamed', { id: automation.id })}
          onClick={onRun}
          size="sm"
          type="button"
          variant="outline"
        >
          <Play className="size-4" />
          {t('automation.runNow')}
        </Button>
        <Button
          aria-label={t('automation.deleteNamed', { id: automation.id })}
          onClick={onDelete}
          size="sm"
          type="button"
          variant="outline"
        >
          <Trash2 className="size-4" />
          {t('automation.delete')}
        </Button>
      </div>
    </article>
  )
}

function Detail({ children, label }: { children: string; label: string }) {
  return (
    <div>
      <span className="block text-muted-foreground text-xs">{label}</span>
      <span className="mt-0.5 block font-medium">{children}</span>
    </div>
  )
}

function Field({
  children,
  fieldId,
  label,
}: {
  children: ReactNode
  fieldId: string
  label: string
}) {
  return (
    <div className="space-y-2">
      <label className="block font-medium text-sm" htmlFor={fieldId}>
        {label}
      </label>
      {children}
    </div>
  )
}

function simpleToolProfileLabel(toolProfile: ToolProfile, t: (key: string) => string) {
  if (typeof toolProfile === 'string') {
    return t(`automation.toolProfile.${toolProfile}`)
  }

  return t('automation.toolProfile.custom')
}

function runStatusBadgeVariant(status: AutomationRunRecord['status']) {
  return status === 'failed' ? 'destructive' : 'outline'
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}

const inputClassName =
  'h-10 w-full rounded-sm border border-border bg-surface px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring'
