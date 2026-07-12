import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlarmClock, Play, Save, Trash2 } from 'lucide-react'
import { type FormEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  AutomationRunRecord,
  AutomationSpec,
  PermissionMode,
  ToolProfile,
} from '@/generated/daemon-protocol'
import { hasObviousUnredactedSecret } from '@/shared/tauri/commands'
import { useDaemonClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import { FieldControl } from '@/shared/ui/field'
import { Input } from '@/shared/ui/input'
import { Section, SectionDescription, SectionHeader, SectionTitle } from '@/shared/ui/section'
import { Select } from '@/shared/ui/select'
import { StatusBadge, type StatusBadgeProps } from '@/shared/ui/status-badge'
import { Switch } from '@/shared/ui/switch'
import { Textarea } from '@/shared/ui/textarea'

const automationQueryKeys = {
  all: ['automations'] as const,
  list: (workspaceRoot: string | undefined) =>
    [...automationQueryKeys.all, workspaceRoot ?? null, 'list'] as const,
}

const automationRunQueryKeys = {
  all: ['automation-runs'] as const,
  list: (workspaceRoot: string | undefined) =>
    [...automationRunQueryKeys.all, workspaceRoot ?? null, 'list'] as const,
}

const toolProfileOptions = ['minimal', 'coding', 'full'] as const
type PresetToolProfile = Extract<ToolProfile, (typeof toolProfileOptions)[number]>

const permissionModeOptions = ['default', 'auto', 'bypass_permissions'] as const
const missedRunPolicyOptions = ['skip', 'run_once'] as const
const automationIdPattern = /^[A-Za-z0-9][A-Za-z0-9._-]*$/

export function AutomationSettings({ workspaceRoot }: { workspaceRoot?: string }) {
  const { t } = useTranslation('settings')
  const daemonClient = useDaemonClient()
  const queryClient = useQueryClient()
  const formRef = useRef<HTMLFormElement>(null)
  const [formError, setFormError] = useState<string | null>(null)
  const [operationError, setOperationError] = useState<string | null>(null)
  const automationsQuery = useQuery({
    queryKey: automationQueryKeys.list(workspaceRoot),
    queryFn: () => daemonClient.listAutomations(workspaceRoot),
  })
  const runsQuery = useQuery({
    queryKey: automationRunQueryKeys.list(workspaceRoot),
    queryFn: () => daemonClient.listAutomationRuns(workspaceRoot),
  })
  const saveMutation = useMutation({
    mutationFn: (automation: AutomationSpec) =>
      daemonClient.saveAutomation(workspaceRoot, automation),
    onSuccess: async () => {
      formRef.current?.reset()
      setFormError(null)
      await queryClient.invalidateQueries({ queryKey: automationQueryKeys.all })
    },
  })
  const toggleMutation = useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      daemonClient.setAutomationEnabled(workspaceRoot, id, enabled),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: automationQueryKeys.all })
    },
  })
  const runNowMutation = useMutation({
    mutationFn: (id: string) => daemonClient.runAutomationNow(workspaceRoot, id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: automationRunQueryKeys.all })
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => daemonClient.deleteAutomation(workspaceRoot, id),
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
    <Section>
      <SectionHeader className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <AlarmClock className="size-4" />
        </div>
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <SectionTitle>{t('automation.title')}</SectionTitle>
            <Badge variant="outline">{t('scope.globalDefaults')}</Badge>
          </div>
          <SectionDescription>{t('automation.description')}</SectionDescription>
        </div>
      </SectionHeader>

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

            <div className="space-y-3 rounded-md border border-border bg-background p-4">
              <h3 className="font-medium text-sm">{t('automation.form.title')}</h3>
              <form className="space-y-4" onSubmit={submit} ref={formRef}>
                <div className="grid gap-3 md:grid-cols-2">
                  <FieldControl fieldId="automation-id" label={t('automation.form.id')}>
                    <Input id="automation-id" name="id" placeholder="nightly-checks" />
                  </FieldControl>
                  <FieldControl fieldId="automation-interval" label={t('automation.form.interval')}>
                    <Input
                      defaultValue={30}
                      id="automation-interval"
                      min={1}
                      name="intervalMinutes"
                      type="number"
                    />
                  </FieldControl>
                  <FieldControl
                    fieldId="automation-tool-profile"
                    label={t('automation.form.toolProfile')}
                  >
                    <Select defaultValue="coding" id="automation-tool-profile" name="toolProfile">
                      {toolProfileOptions.map((toolProfile) => (
                        <option key={toolProfile} value={toolProfile}>
                          {t(`automation.toolProfile.${toolProfile}`)}
                        </option>
                      ))}
                    </Select>
                  </FieldControl>
                  <FieldControl
                    fieldId="automation-permission-mode"
                    label={t('automation.form.permissionMode')}
                  >
                    <Select
                      defaultValue="default"
                      id="automation-permission-mode"
                      name="permissionMode"
                    >
                      {permissionModeOptions.map((permissionMode) => (
                        <option key={permissionMode} value={permissionMode}>
                          {t(`automation.permissionMode.${permissionMode}`)}
                        </option>
                      ))}
                    </Select>
                  </FieldControl>
                  <FieldControl
                    fieldId="automation-missed-policy"
                    label={t('automation.form.missedRunPolicy')}
                  >
                    <Select
                      defaultValue="skip"
                      id="automation-missed-policy"
                      name="missedRunPolicy"
                    >
                      {missedRunPolicyOptions.map((policy) => (
                        <option key={policy} value={policy}>
                          {t(`automation.missedRunPolicy.${policy}`)}
                        </option>
                      ))}
                    </Select>
                  </FieldControl>
                  <label
                    className="flex items-center gap-2 self-end text-sm"
                    htmlFor="automation-enabled"
                  >
                    <Checkbox id="automation-enabled" name="enabled" />
                    {t('automation.form.enabled')}
                  </label>
                  <div className="md:col-span-2">
                    <FieldControl fieldId="automation-prompt" label={t('automation.form.prompt')}>
                      <Textarea
                        id="automation-prompt"
                        name="prompt"
                        placeholder={t('automation.form.promptPlaceholder')}
                      />
                    </FieldControl>
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
                      <StatusBadge tone={runStatusTone(run.status)}>{run.status}</StatusBadge>
                    </div>
                    <p className="text-muted-foreground text-xs">{formatDate(run.startedAt)}</p>
                  </div>
                ))}
              </div>
            ) : null}
          </aside>
        </div>
      ) : null}
    </Section>
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
            <StatusBadge tone={automation.enabled ? 'success' : 'neutral'}>
              {automation.enabled ? t('automation.enabled') : t('automation.disabled')}
            </StatusBadge>
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

function simpleToolProfileLabel(toolProfile: ToolProfile, t: (key: string) => string) {
  if (typeof toolProfile === 'string') {
    return t(`automation.toolProfile.${toolProfile}`)
  }

  return t('automation.toolProfile.custom')
}

function runStatusTone(status: AutomationRunRecord['status']): StatusBadgeProps['tone'] {
  return status === 'failed' ? 'destructive' : 'neutral'
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}
