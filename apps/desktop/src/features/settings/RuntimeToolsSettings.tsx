import { useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, RotateCcw, Search, Wrench, XCircle } from 'lucide-react'
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  getRuntimeExecutionStatus,
  type ListRuntimeToolsResponse,
  listRuntimeTools,
  type RuntimeExecutionStatus,
  type RuntimeToolSummary,
  resetRuntimeTools,
  setRuntimeToolEnabled,
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
import { Switch } from '@/shared/ui/switch'

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
            disabled={!toolsQuery.data?.customized || resetting || pendingToolName !== null}
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
              group={group}
              key={group}
              localizedDescription={localizedDescription}
              pendingToolName={pendingToolName}
              requestToolChange={requestToolChange}
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
  group,
  localizedDescription,
  pendingToolName,
  requestToolChange,
  tools,
}: {
  group: string
  localizedDescription: (tool: RuntimeToolSummary) => string
  pendingToolName: string | null
  requestToolChange: (tool: RuntimeToolSummary, enabled: boolean) => void
  tools: RuntimeToolSummary[]
}) {
  const { t } = useTranslation('skills')
  const enabled = tools.filter((tool) => tool.configuredEnabled).length

  return (
    <section className="border-border border-b last:border-b-0">
      <div className="flex items-center justify-between bg-background/70 px-5 py-2.5">
        <h3 className="font-medium text-sm">{group}</h3>
        <span className="text-muted-foreground text-xs">
          {t('tools.groupCount', { enabled, total: tools.length })}
        </span>
      </div>
      <div>
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
            <div className="flex items-center">
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
