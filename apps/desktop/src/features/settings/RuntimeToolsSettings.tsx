import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  RotateCcw,
  Search,
  Settings2,
  Wrench,
  XCircle,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  getRuntimeExecutionStatus,
  type ListRuntimeToolsResponse,
  listRuntimeTools,
  type RuntimeExecutionStatus,
  type RuntimeToolSummary,
  resetRuntimeToolConfig,
  resetRuntimeTools,
  setRuntimeToolEnabled,
  updateRuntimeToolConfig,
} from '@/shared/tauri/commands'
import { getCommandErrorMessage } from '@/shared/tauri/errors'
import { useCommandClient } from '@/shared/tauri/react'
import { Badge } from '@/shared/ui/badge'
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
import { Switch } from '@/shared/ui/switch'
import { Textarea } from '@/shared/ui/textarea'

type ToolFilter = 'all' | 'enabled' | 'unavailable' | 'risky'

const filters: ToolFilter[] = ['all', 'enabled', 'unavailable', 'risky']

export function RuntimeToolsSettings() {
  const { t } = useTranslation('skills')
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ToolFilter>('all')
  const [pendingToolName, setPendingToolName] = useState<string | null>(null)
  const [resetting, setResetting] = useState(false)
  const [mutationError, setMutationError] = useState<string | null>(null)
  const [confirmationTool, setConfirmationTool] = useState<RuntimeToolSummary | null>(null)
  const [configurationToolName, setConfigurationToolName] = useState<string | null>(null)
  const [pendingConfigurationToolName, setPendingConfigurationToolName] = useState<string | null>(
    null,
  )
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() => new Set())

  const toolsQuery = useQuery({
    queryKey: ['runtime-tools'],
    queryFn: () => listRuntimeTools(commandClient),
  })
  const runtimeQuery = useQuery({
    queryKey: ['settings', 'runtime-execution-status'],
    queryFn: () => getRuntimeExecutionStatus(commandClient),
  })

  const tools = toolsQuery.data?.tools ?? []
  const localizedDescription = useCallback(
    (tool: RuntimeToolSummary) =>
      t(`tools.items.${tool.name}.description`, { defaultValue: tool.description }),
    [t],
  )
  const localizedGroup = useCallback(
    (tool: RuntimeToolSummary) =>
      t(`tools.groups.${tool.group}`, { defaultValue: tool.groupLabel }),
    [t],
  )
  const normalizedQuery = query.trim().toLocaleLowerCase()
  const filteredTools = useMemo(
    () =>
      tools.filter((tool) => {
        const matchesFilter =
          filter === 'all' ||
          (filter === 'enabled' && tool.configuredEnabled) ||
          (filter === 'unavailable' && tool.configuredEnabled && !tool.available) ||
          (filter === 'risky' && tool.access === 'destructive')
        if (!matchesFilter) return false
        if (!normalizedQuery) return true
        return [
          tool.name,
          tool.displayName,
          localizedDescription(tool),
          localizedGroup(tool),
          t(`tools.origin.${tool.originKind}`),
          tool.originId ?? '',
        ]
          .join(' ')
          .toLocaleLowerCase()
          .includes(normalizedQuery)
      }),
    [filter, localizedDescription, localizedGroup, normalizedQuery, t, tools],
  )
  const groups = useMemo(
    () => groupTools(filteredTools, localizedGroup),
    [filteredTools, localizedGroup],
  )
  const enabledCount = tools.filter((tool) => tool.configuredEnabled).length
  const configurationTool = tools.find((tool) => tool.name === configurationToolName) ?? null

  async function updateTool(tool: RuntimeToolSummary, enabled: boolean) {
    setPendingToolName(tool.name)
    setMutationError(null)
    try {
      const response = await setRuntimeToolEnabled({ enabled, name: tool.name }, commandClient)
      queryClient.setQueryData<ListRuntimeToolsResponse>(['runtime-tools'], response)
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
    } finally {
      setPendingToolName(null)
    }
  }

  function requestToolChange(tool: RuntimeToolSummary, enabled: boolean) {
    if (enabled && tool.access === 'destructive') {
      setConfirmationTool(tool)
      return
    }
    void updateTool(tool, enabled)
  }

  async function restoreDefaults() {
    setResetting(true)
    setMutationError(null)
    try {
      const response = await resetRuntimeTools(commandClient)
      queryClient.setQueryData<ListRuntimeToolsResponse>(['runtime-tools'], response)
    } catch (error) {
      setMutationError(getCommandErrorMessage(error))
    } finally {
      setResetting(false)
    }
  }

  async function saveConfiguration(
    tool: RuntimeToolSummary,
    timeoutMs: number,
    parameters: Record<string, unknown>,
  ) {
    setPendingConfigurationToolName(tool.name)
    try {
      const response = await updateRuntimeToolConfig(
        { name: tool.name, parameters, timeoutMs },
        commandClient,
      )
      queryClient.setQueryData<ListRuntimeToolsResponse>(['runtime-tools'], response)
    } finally {
      setPendingConfigurationToolName(null)
    }
  }

  async function restoreToolConfiguration(tool: RuntimeToolSummary) {
    setPendingConfigurationToolName(tool.name)
    try {
      const response = await resetRuntimeToolConfig({ name: tool.name }, commandClient)
      queryClient.setQueryData<ListRuntimeToolsResponse>(['runtime-tools'], response)
    } finally {
      setPendingConfigurationToolName(null)
    }
  }

  return (
    <section className="overflow-hidden rounded-md border border-border bg-surface">
      <header className="flex flex-col gap-4 border-border border-b p-5 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex items-start gap-3">
          <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
            <Wrench className="size-4" />
          </div>
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="font-semibold text-base">{t('tools.title')}</h2>
              {toolsQuery.data ? (
                <Badge variant="outline">{t(`tools.scope.${toolsQuery.data.scope}`)}</Badge>
              ) : null}
            </div>
            <p className="mt-1 text-muted-foreground text-sm">{t('tools.description')}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-3 sm:justify-end">
          <span className="text-muted-foreground text-sm">
            {t('tools.enabledCount', { enabled: enabledCount, total: tools.length })}
          </span>
          <Button
            disabled={
              !toolsQuery.data?.customized ||
              resetting ||
              pendingToolName !== null ||
              pendingConfigurationToolName !== null
            }
            onClick={() => void restoreDefaults()}
            size="sm"
            variant="outline"
          >
            <RotateCcw data-icon />
            {resetting ? t('tools.resetting') : t('tools.reset')}
          </Button>
        </div>
      </header>

      <RuntimeSummary
        enabledTools={tools.filter((tool) => tool.configuredEnabled)}
        error={runtimeQuery.isError ? getCommandErrorMessage(runtimeQuery.error) : null}
        loading={runtimeQuery.isLoading}
        status={runtimeQuery.data}
      />

      <div className="space-y-3 border-border border-b p-4">
        <div className="relative">
          <Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-3 size-4 text-muted-foreground" />
          <Input
            aria-label={t('tools.searchLabel')}
            className="pl-9"
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('tools.searchPlaceholder')}
            value={query}
          />
        </div>
        <fieldset className="flex min-w-0 flex-wrap gap-1 border-0 p-0">
          <legend className="sr-only">{t('tools.filters.label')}</legend>
          {filters.map((value) => (
            <Button
              aria-pressed={filter === value}
              key={value}
              onClick={() => setFilter(value)}
              size="sm"
              variant={filter === value ? 'secondary' : 'ghost'}
            >
              {t(`tools.filters.${value}`)}
            </Button>
          ))}
          {(normalizedQuery || filter !== 'all') && tools.length > 0 ? (
            <span className="ml-auto self-center text-muted-foreground text-xs">
              {t('tools.matchCount', { count: filteredTools.length, total: tools.length })}
            </span>
          ) : null}
        </fieldset>
      </div>

      {mutationError ? (
        <div className="flex items-start gap-2 border-border border-b px-5 py-3 text-destructive text-sm">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          <span>{mutationError}</span>
        </div>
      ) : null}

      {toolsQuery.isLoading ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.loading')}</p>
      ) : toolsQuery.isError ? (
        <p className="p-5 text-destructive text-sm">{getCommandErrorMessage(toolsQuery.error)}</p>
      ) : groups.length === 0 ? (
        <p className="p-5 text-muted-foreground text-sm">{t('tools.empty')}</p>
      ) : (
        <div>
          {groups.map(([group, groupTools]) => (
            <ToolGroup
              collapsed={collapsedGroups.has(group)}
              group={group}
              key={group}
              localizedDescription={localizedDescription}
              onConfigure={setConfigurationToolName}
              pendingToolName={pendingToolName}
              requestToolChange={requestToolChange}
              toggleCollapsed={() => {
                setCollapsedGroups((current) => {
                  const next = new Set(current)
                  if (next.has(group)) next.delete(group)
                  else next.add(group)
                  return next
                })
              }}
              tools={groupTools}
            />
          ))}
        </div>
      )}

      <Dialog
        onOpenChange={(open) => {
          if (!open) setConfirmationTool(null)
        }}
        open={confirmationTool !== null}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('tools.confirm.title')}</DialogTitle>
            <DialogDescription>
              {t('tools.confirm.description', { name: confirmationTool?.displayName ?? '' })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button onClick={() => setConfirmationTool(null)} variant="outline">
              {t('tools.confirm.cancel')}
            </Button>
            <Button
              onClick={() => {
                const tool = confirmationTool
                setConfirmationTool(null)
                if (tool) void updateTool(tool, true)
              }}
              variant="destructive"
            >
              {t('tools.confirm.enable')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ToolConfigurationDialog
        onOpenChange={(open) => {
          if (!open) setConfigurationToolName(null)
        }}
        onReset={restoreToolConfiguration}
        onSave={saveConfiguration}
        pending={pendingConfigurationToolName !== null}
        tool={configurationTool}
      />
    </section>
  )
}

function RuntimeSummary({
  enabledTools,
  error,
  loading,
  status,
}: {
  enabledTools: RuntimeToolSummary[]
  error: string | null
  loading: boolean
  status?: RuntimeExecutionStatus
}) {
  const { t } = useTranslation('skills')
  const executableCount = enabledTools.filter((tool) => tool.available).length

  if (loading) {
    return (
      <p className="border-border border-b px-5 py-3 text-muted-foreground text-sm">
        {t('tools.runtime.loading')}
      </p>
    )
  }
  if (error || !status) {
    return (
      <div className="border-border border-b px-5 py-3 text-destructive text-sm">
        {error ?? t('tools.runtime.unavailable')}
      </div>
    )
  }

  const reasons = [...status.processSandbox.unavailableReasons, ...status.httpBroker.deniedReasons]

  return (
    <details className="group border-border border-b px-5 py-3 text-sm">
      <summary className="flex cursor-pointer list-none flex-wrap items-center gap-x-2 gap-y-1 text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <span className="font-medium text-foreground">{t('tools.runtime.title')}</span>
        <span>·</span>
        <span>{t('tools.runtime.sandbox', { backend: status.processSandbox.backendId })}</span>
        <span>·</span>
        <span>
          {status.httpBroker.available
            ? t('tools.runtime.networkAvailable')
            : t('tools.runtime.networkUnavailable')}
        </span>
        <span>·</span>
        <span>
          {t('tools.runtime.executable', {
            available: executableCount,
            enabled: enabledTools.length,
          })}
        </span>
        <span className="ml-auto text-xs group-open:hidden">{t('tools.runtime.showDetails')}</span>
        <span className="ml-auto hidden text-xs group-open:inline">
          {t('tools.runtime.hideDetails')}
        </span>
      </summary>
      <div className="mt-3 grid gap-3 rounded-md bg-background p-3 text-xs sm:grid-cols-2">
        <div>
          <div className="font-medium text-muted-foreground">{t('tools.runtime.candidates')}</div>
          <div className="mt-1 font-mono">
            {status.processSandbox.candidateIds.join(', ') || '—'}
          </div>
        </div>
        <div>
          <div className="font-medium text-muted-foreground">{t('tools.runtime.policies')}</div>
          <div className="mt-1 font-mono">
            {[
              ...status.processSandbox.availableNetworkPolicies,
              ...status.processSandbox.availableWorkspacePolicies,
            ].join(', ') || '—'}
          </div>
        </div>
        {reasons.length > 0 ? (
          <div className="sm:col-span-2">
            <div className="font-medium text-muted-foreground">{t('tools.runtime.reasons')}</div>
            <ul className="mt-1 space-y-1 text-muted-foreground">
              {reasons.map((reason) => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          </div>
        ) : null}
      </div>
    </details>
  )
}

function ToolGroup({
  collapsed,
  group,
  localizedDescription,
  onConfigure,
  pendingToolName,
  requestToolChange,
  toggleCollapsed,
  tools,
}: {
  collapsed: boolean
  group: string
  localizedDescription: (tool: RuntimeToolSummary) => string
  onConfigure: (toolName: string) => void
  pendingToolName: string | null
  requestToolChange: (tool: RuntimeToolSummary, enabled: boolean) => void
  toggleCollapsed: () => void
  tools: RuntimeToolSummary[]
}) {
  const { t } = useTranslation('skills')
  const enabled = tools.filter((tool) => tool.configuredEnabled).length

  return (
    <section className="border-border border-b last:border-b-0">
      <h3>
        <button
          aria-expanded={!collapsed}
          className="flex w-full items-center gap-2 bg-background/70 px-5 py-2.5 text-left outline-none transition-colors hover:bg-muted/70 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={toggleCollapsed}
          type="button"
        >
          <ChevronDown
            aria-hidden="true"
            className={`size-4 text-muted-foreground transition-transform ${collapsed ? '-rotate-90' : ''}`}
          />
          <span className="font-medium text-sm">{group}</span>
          <span className="ml-auto text-muted-foreground text-xs">
            {t('tools.groupCount', { enabled, total: tools.length })}
          </span>
        </button>
      </h3>
      <div hidden={collapsed}>
        {tools.map((tool) => (
          <div
            className="grid grid-cols-[minmax(0,1fr)_auto] gap-4 border-border border-t px-5 py-4 first:border-t-0"
            key={tool.name}
          >
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium text-sm">{tool.displayName}</span>
                {tool.name !== tool.displayName ? (
                  <span className="font-mono text-muted-foreground text-xs">{tool.name}</span>
                ) : null}
                <Badge variant={accessBadgeVariant(tool.access)}>
                  {t(`tools.access.${tool.access}`)}
                </Badge>
                <Badge variant="outline">{t(`tools.origin.${tool.originKind}`)}</Badge>
                <ToolState tool={tool} />
              </div>
              <p className="mt-1.5 max-w-3xl text-muted-foreground text-sm leading-5">
                {localizedDescription(tool)}
              </p>
              <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground text-xs">
                <span>{t(`tools.execution.${tool.executionChannel}`)}</span>
                {tool.originId ? <span className="font-mono">{tool.originId}</span> : null}
                {tool.longRunning ? <span>{t('tools.longRunning')}</span> : null}
                {tool.unavailableReason && tool.configuredEnabled ? (
                  <span className="text-destructive">{tool.unavailableReason}</span>
                ) : null}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                aria-label={t('tools.config.openLabel', { name: tool.displayName })}
                onClick={() => onConfigure(tool.name)}
                size="icon"
                title={t('tools.config.open')}
                variant={tool.configurationCustomized ? 'secondary' : 'ghost'}
              >
                <Settings2 className="size-4" />
              </Button>
              <Switch
                aria-label={t('tools.toggleLabel', { name: tool.displayName })}
                checked={tool.configuredEnabled}
                disabled={pendingToolName !== null}
                onCheckedChange={(checked) => requestToolChange(tool, checked)}
              />
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}

type ToolConfigurationProperty = {
  type?: string
  title?: string
  description?: string
  enum?: unknown[]
  minimum?: number
  maximum?: number
}

function ToolConfigurationDialog({
  onOpenChange,
  onReset,
  onSave,
  pending,
  tool,
}: {
  onOpenChange: (open: boolean) => void
  onReset: (tool: RuntimeToolSummary) => Promise<void>
  onSave: (
    tool: RuntimeToolSummary,
    timeoutMs: number,
    parameters: Record<string, unknown>,
  ) => Promise<void>
  pending: boolean
  tool: RuntimeToolSummary | null
}) {
  const { t } = useTranslation('skills')
  const [timeoutSeconds, setTimeoutSeconds] = useState('120')
  const [parameters, setParameters] = useState<Record<string, unknown>>({})
  const [parametersJson, setParametersJson] = useState('{}')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!tool) return
    const nextParameters = { ...tool.parameters }
    setTimeoutSeconds(String(Math.round(tool.timeoutMs / 1_000)))
    setParameters(nextParameters)
    setParametersJson(JSON.stringify(nextParameters, null, 2))
    setError(null)
  }, [tool])

  const properties = configurationProperties(tool)
  const usesJsonEditor = properties.some(([, property]) => !isSimpleConfigurationProperty(property))

  function updateParameter(name: string, value: unknown) {
    setParameters((current) => {
      const next = { ...current, [name]: value }
      setParametersJson(JSON.stringify(next, null, 2))
      return next
    })
  }

  async function save() {
    if (!tool) return
    setError(null)
    try {
      const seconds = Number(timeoutSeconds)
      if (!Number.isInteger(seconds) || seconds < 1 || seconds > 86_400) {
        throw new Error(t('tools.config.timeoutInvalid'))
      }
      let nextParameters = parameters
      if (usesJsonEditor) {
        const parsed = JSON.parse(parametersJson) as unknown
        if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
          throw new Error(t('tools.config.parametersInvalid'))
        }
        nextParameters = parsed as Record<string, unknown>
      }
      await onSave(tool, seconds * 1_000, nextParameters)
      onOpenChange(false)
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : getCommandErrorMessage(saveError))
    }
  }

  async function reset() {
    if (!tool) return
    setError(null)
    try {
      await onReset(tool)
    } catch (resetError) {
      setError(
        resetError instanceof Error ? resetError.message : getCommandErrorMessage(resetError),
      )
    }
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={tool !== null}>
      <DialogContent className="max-h-[min(720px,85vh)] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t('tools.config.title', { name: tool?.displayName ?? '' })}</DialogTitle>
          <DialogDescription>{t('tools.config.description')}</DialogDescription>
        </DialogHeader>

        {tool ? (
          <div className="space-y-5 py-1">
            <label className="block space-y-2 text-sm" htmlFor="tool-timeout-seconds">
              <span className="font-medium">{t('tools.config.timeout')}</span>
              <div className="flex items-center gap-2">
                <Input
                  id="tool-timeout-seconds"
                  max={86_400}
                  min={1}
                  onChange={(event) => setTimeoutSeconds(event.target.value)}
                  type="number"
                  value={timeoutSeconds}
                />
                <span className="shrink-0 text-muted-foreground text-sm">
                  {t('tools.config.seconds')}
                </span>
              </div>
              <span className="block text-muted-foreground text-xs">
                {t('tools.config.timeoutHelp', {
                  seconds: Math.round(tool.defaultTimeoutMs / 1_000),
                })}
              </span>
            </label>

            <div className="border-border border-t pt-4">
              <div className="mb-3">
                <div className="font-medium text-sm">{t('tools.config.parameters')}</div>
                <p className="mt-1 text-muted-foreground text-xs">
                  {t('tools.config.parametersHelp')}
                </p>
              </div>
              {properties.length === 0 ? (
                <p className="rounded-md bg-background px-3 py-2 text-muted-foreground text-sm">
                  {t('tools.config.noParameters')}
                </p>
              ) : usesJsonEditor ? (
                <label className="block space-y-2 text-sm" htmlFor="tool-parameters-json">
                  <span className="font-medium">{t('tools.config.jsonParameters')}</span>
                  <Textarea
                    className="min-h-44 font-mono text-xs"
                    id="tool-parameters-json"
                    onChange={(event) => setParametersJson(event.target.value)}
                    value={parametersJson}
                  />
                </label>
              ) : (
                <div className="space-y-4">
                  {properties.map(([name, property]) => (
                    <ConfigurationField
                      key={name}
                      name={name}
                      onChange={(value) => updateParameter(name, value)}
                      property={property}
                      toolName={tool.name}
                      value={parameters[name]}
                    />
                  ))}
                </div>
              )}
            </div>

            {error ? (
              <div className="flex items-start gap-2 text-destructive text-sm">
                <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                <span>{error}</span>
              </div>
            ) : null}
          </div>
        ) : null}

        <DialogFooter className="sm:justify-between">
          <Button
            disabled={!tool?.configurationCustomized || pending}
            onClick={() => void reset()}
            variant="ghost"
          >
            <RotateCcw data-icon />
            {t('tools.config.reset')}
          </Button>
          <div className="flex gap-2">
            <Button disabled={pending} onClick={() => onOpenChange(false)} variant="outline">
              {t('tools.config.cancel')}
            </Button>
            <Button disabled={pending} onClick={() => void save()}>
              {pending ? t('tools.config.saving') : t('tools.config.save')}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ConfigurationField({
  name,
  onChange,
  property,
  toolName,
  value,
}: {
  name: string
  onChange: (value: unknown) => void
  property: ToolConfigurationProperty
  toolName: string
  value: unknown
}) {
  const { t } = useTranslation('skills')
  const label = t(`tools.config.fields.${toolName}.${name}.label`, {
    defaultValue: property.title ?? name,
  })
  const description = t(`tools.config.fields.${toolName}.${name}.description`, {
    defaultValue: property.description ?? '',
  })
  const id = `tool-config-${toolName}-${name}`

  if (property.type === 'boolean') {
    return (
      <div className="flex items-start justify-between gap-4">
        <div>
          <label className="font-medium text-sm" htmlFor={id}>
            {label}
          </label>
          {description ? <p className="mt-1 text-muted-foreground text-xs">{description}</p> : null}
        </div>
        <Switch checked={value === true} id={id} onCheckedChange={onChange} />
      </div>
    )
  }

  if (property.enum?.every((option) => typeof option === 'string')) {
    return (
      <label className="block space-y-2 text-sm" htmlFor={id}>
        <span className="font-medium">{label}</span>
        <Select
          id={id}
          onChange={(event) => onChange(event.target.value)}
          value={String(value ?? '')}
        >
          {property.enum.map((option) => (
            <option key={String(option)} value={String(option)}>
              {String(option)}
            </option>
          ))}
        </Select>
        {description ? (
          <span className="block text-muted-foreground text-xs">{description}</span>
        ) : null}
      </label>
    )
  }

  const numeric = property.type === 'integer' || property.type === 'number'
  return (
    <label className="block space-y-2 text-sm" htmlFor={id}>
      <span className="font-medium">{label}</span>
      <Input
        id={id}
        max={property.maximum}
        min={property.minimum}
        onChange={(event) => onChange(numeric ? Number(event.target.value) : event.target.value)}
        step={property.type === 'number' ? 'any' : undefined}
        type={numeric ? 'number' : 'text'}
        value={typeof value === 'string' || typeof value === 'number' ? value : ''}
      />
      {description ? (
        <span className="block text-muted-foreground text-xs">{description}</span>
      ) : null}
    </label>
  )
}

function configurationProperties(
  tool: RuntimeToolSummary | null,
): Array<[string, ToolConfigurationProperty]> {
  const raw = tool?.configurationSchema?.properties
  if (!raw || Array.isArray(raw) || typeof raw !== 'object') return []
  return Object.entries(raw).filter(
    (entry): entry is [string, ToolConfigurationProperty] =>
      Boolean(entry[1]) && !Array.isArray(entry[1]) && typeof entry[1] === 'object',
  )
}

function isSimpleConfigurationProperty(property: ToolConfigurationProperty) {
  return (
    property.type === 'string' ||
    property.type === 'number' ||
    property.type === 'integer' ||
    property.type === 'boolean'
  )
}

function ToolState({ tool }: { tool: RuntimeToolSummary }) {
  const { t } = useTranslation('skills')
  if (!tool.configuredEnabled) {
    return (
      <span className="inline-flex items-center gap-1 text-muted-foreground text-xs">
        <XCircle className="size-3.5" />
        {t('tools.status.disabled')}
      </span>
    )
  }
  if (!tool.available) {
    return (
      <span className="inline-flex items-center gap-1 text-destructive text-xs">
        <AlertTriangle className="size-3.5" />
        {t('tools.status.unavailable')}
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1 text-success text-xs">
      <CheckCircle2 className="size-3.5" />
      {t('tools.status.available')}
    </span>
  )
}

function groupTools(
  tools: RuntimeToolSummary[],
  labelForTool: (tool: RuntimeToolSummary) => string,
) {
  const groups = new Map<string, RuntimeToolSummary[]>()
  for (const tool of tools) {
    const label = labelForTool(tool)
    const group = groups.get(label) ?? []
    group.push(tool)
    groups.set(label, group)
  }
  return Array.from(groups.entries())
}

function accessBadgeVariant(access: RuntimeToolSummary['access']) {
  if (access === 'destructive') return 'destructive'
  if (access === 'readOnly') return 'secondary'
  return 'outline'
}
