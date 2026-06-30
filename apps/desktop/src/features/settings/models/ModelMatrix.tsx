import { Gauge, RefreshCw, Wifi } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

import {
  isFailingConnectivity,
  type ModelAssetRow,
  type QuotaDisplayState,
  type UsageDisplayState,
} from './model-settings-view-model'

type ModelMatrixProps = {
  isProbePending: (configId: string) => boolean
  isQuotaRefreshPending: (configId: string) => boolean
  onProbe: (configId: string) => void
  onRefreshQuota: (configId: string) => void
  rows: ModelAssetRow[]
}

export function ModelMatrix({
  isProbePending,
  isQuotaRefreshPending,
  onProbe,
  onRefreshQuota,
  rows,
}: ModelMatrixProps) {
  const { t } = useTranslation('settings')

  if (rows.length === 0) {
    return (
      <section
        aria-label={t('models.matrix.label')}
        className="rounded-md border border-dashed border-border bg-surface p-8 text-center"
      >
        <h2 className="font-semibold text-base">{t('models.empty.title')}</h2>
        <p className="mt-2 text-muted-foreground text-sm">{t('models.empty.description')}</p>
      </section>
    )
  }

  return (
    <TooltipProvider>
      <section aria-label={t('models.matrix.label')} className="min-w-0">
        <table className="hidden w-full min-w-[980px] border-separate border-spacing-0 text-left text-sm min-[1100px]:table">
          <thead>
            <tr className="text-muted-foreground text-xs">
              <HeaderCell>{t('models.columns.identity')}</HeaderCell>
              <HeaderCell>{t('models.columns.provider')}</HeaderCell>
              <HeaderCell>{t('models.columns.default')}</HeaderCell>
              <HeaderCell>{t('models.columns.connectivity')}</HeaderCell>
              <HeaderCell>{t('models.columns.latency')}</HeaderCell>
              <HeaderCell>{t('models.columns.timeout')}</HeaderCell>
              <HeaderCell className="max-lg:hidden">{t('models.columns.todayUsage')}</HeaderCell>
              <HeaderCell className="max-lg:hidden">{t('models.columns.monthUsage')}</HeaderCell>
              <HeaderCell className="max-lg:hidden">{t('models.columns.totalUsage')}</HeaderCell>
              <HeaderCell>{t('models.columns.quota')}</HeaderCell>
              <HeaderCell>{t('models.columns.actions')}</HeaderCell>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const probePending = isProbePending(row.configId)
              const quotaPending = isQuotaRefreshPending(row.configId)

              return (
                <tr key={row.configId} className="group">
                  <BodyCell className="rounded-l-md border-l">
                    <div className="min-w-0">
                      <div className="truncate font-medium">{row.displayName}</div>
                      <div className="truncate text-muted-foreground text-xs">{row.modelId}</div>
                    </div>
                  </BodyCell>
                  <BodyCell>{row.providerDisplayName}</BodyCell>
                  <BodyCell>
                    {row.isDefault ? (
                      <Badge>{t('models.defaultMarker')}</Badge>
                    ) : (
                      <span className="text-muted-foreground">-</span>
                    )}
                  </BodyCell>
                  <BodyCell>
                    <ConnectivityBadge row={row} />
                  </BodyCell>
                  <BodyCell>{formatLatency(row, t('models.unavailable'))}</BodyCell>
                  <BodyCell>{formatTimeout(row)}</BodyCell>
                  <BodyCell className="max-lg:hidden">
                    {formatUsage(row.usage, 'today', t('models.unavailable'))}
                  </BodyCell>
                  <BodyCell className="max-lg:hidden">
                    {formatUsage(row.usage, 'monthToDate', t('models.unavailable'))}
                  </BodyCell>
                  <BodyCell className="max-lg:hidden">
                    {formatUsage(row.usage, 'allTime', t('models.unavailable'))}
                  </BodyCell>
                  <BodyCell>
                    <QuotaBadge quota={row.quota} />
                  </BodyCell>
                  <BodyCell className="rounded-r-md border-r">
                    <div className="flex items-center gap-1.5">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            aria-label={
                              probePending
                                ? t('models.actions.probing', { name: row.displayName })
                                : t('models.actions.probe', { name: row.displayName })
                            }
                            disabled={probePending}
                            onClick={() => onProbe(row.configId)}
                            size="icon"
                            type="button"
                            variant="outline"
                          >
                            <Wifi aria-hidden="true" className="size-4" data-icon />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {probePending
                            ? t('models.actions.probingShort')
                            : t('models.actions.probeShort')}
                        </TooltipContent>
                      </Tooltip>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            aria-label={
                              quotaPending
                                ? t('models.actions.refreshingQuota', { name: row.displayName })
                                : t('models.actions.refreshQuota', { name: row.displayName })
                            }
                            disabled={quotaPending}
                            onClick={() => onRefreshQuota(row.configId)}
                            size="icon"
                            type="button"
                            variant="outline"
                          >
                            {quotaPending ? (
                              <RefreshCw
                                aria-hidden="true"
                                className="size-4 animate-spin"
                                data-icon
                              />
                            ) : (
                              <Gauge aria-hidden="true" className="size-4" data-icon />
                            )}
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {quotaPending
                            ? t('models.actions.refreshingQuotaShort')
                            : t('models.actions.refreshQuotaShort')}
                        </TooltipContent>
                      </Tooltip>
                    </div>
                  </BodyCell>
                </tr>
              )
            })}
          </tbody>
        </table>

        <ul className="grid gap-3 min-[1100px]:hidden">
          {rows.map((row) => {
            const probePending = isProbePending(row.configId)
            const quotaPending = isQuotaRefreshPending(row.configId)

            return (
              <li
                className="grid gap-3 rounded-md border border-border bg-surface p-3 text-sm"
                key={row.configId}
              >
                <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-3">
                  <div className="min-w-0">
                    <div className="truncate font-medium">{row.displayName}</div>
                    <div className="mt-1 flex min-w-0 flex-wrap items-center gap-1.5 text-muted-foreground text-xs">
                      <span className="truncate">{row.providerDisplayName}</span>
                      <span aria-hidden="true">/</span>
                      <span className="truncate">{row.modelId}</span>
                    </div>
                  </div>
                  <div className="flex shrink-0 items-start gap-1.5">
                    <ProbeButton
                      displayName={row.displayName}
                      isPending={probePending}
                      onProbe={() => onProbe(row.configId)}
                    />
                    <QuotaButton
                      displayName={row.displayName}
                      isPending={quotaPending}
                      onRefreshQuota={() => onRefreshQuota(row.configId)}
                    />
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-2 text-xs">
                  <CompactMetric label={t('models.columns.connectivity')}>
                    <ConnectivityBadge row={row} />
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.timeout')}>
                    {formatTimeout(row)}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.latency')}>
                    {formatLatency(row, t('models.unavailable'))}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.quota')}>
                    <QuotaBadge quota={row.quota} />
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.todayUsage')}>
                    {formatUsage(row.usage, 'today', t('models.unavailable'))}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.monthUsage')}>
                    {formatUsage(row.usage, 'monthToDate', t('models.unavailable'))}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.totalUsage')}>
                    {formatUsage(row.usage, 'allTime', t('models.unavailable'))}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.default')}>
                    {row.isDefault ? (
                      <Badge>{t('models.defaultMarker')}</Badge>
                    ) : (
                      <span className="text-muted-foreground">-</span>
                    )}
                  </CompactMetric>
                </div>
              </li>
            )
          })}
        </ul>
      </section>
    </TooltipProvider>
  )
}

function ProbeButton({
  displayName,
  isPending,
  onProbe,
}: {
  displayName: string
  isPending: boolean
  onProbe: () => void
}) {
  const { t } = useTranslation('settings')

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={
            isPending
              ? t('models.actions.probing', { name: displayName })
              : t('models.actions.probe', { name: displayName })
          }
          disabled={isPending}
          onClick={onProbe}
          size="icon"
          type="button"
          variant="outline"
        >
          <Wifi aria-hidden="true" className="size-4" data-icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {isPending ? t('models.actions.probingShort') : t('models.actions.probeShort')}
      </TooltipContent>
    </Tooltip>
  )
}

function QuotaButton({
  displayName,
  isPending,
  onRefreshQuota,
}: {
  displayName: string
  isPending: boolean
  onRefreshQuota: () => void
}) {
  const { t } = useTranslation('settings')

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={
            isPending
              ? t('models.actions.refreshingQuota', { name: displayName })
              : t('models.actions.refreshQuota', { name: displayName })
          }
          disabled={isPending}
          onClick={onRefreshQuota}
          size="icon"
          type="button"
          variant="outline"
        >
          {isPending ? (
            <RefreshCw aria-hidden="true" className="size-4 animate-spin" data-icon />
          ) : (
            <Gauge aria-hidden="true" className="size-4" data-icon />
          )}
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {isPending
          ? t('models.actions.refreshingQuotaShort')
          : t('models.actions.refreshQuotaShort')}
      </TooltipContent>
    </Tooltip>
  )
}

function CompactMetric({ children, label }: { children: ReactNode; label: string }) {
  return (
    <div className="min-w-0 rounded-sm border border-border bg-background px-2 py-1.5">
      <div className="truncate text-muted-foreground">{label}</div>
      <div className="mt-1 min-h-5 truncate font-medium">{children}</div>
    </div>
  )
}

function HeaderCell({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <th className={`border-border border-b px-3 py-2 font-medium ${className ?? ''}`}>
      {children}
    </th>
  )
}

function BodyCell({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <td className={`border-border border-b bg-surface px-3 py-3 align-middle ${className ?? ''}`}>
      {children}
    </td>
  )
}

function ConnectivityBadge({ row }: { row: ModelAssetRow }) {
  const { t } = useTranslation('settings')
  const status = row.connectivity.status

  if (status === 'online') {
    return <Badge variant="success">{t('models.connectivity.online')}</Badge>
  }
  if (status === 'never_checked') {
    return <Badge variant="outline">{t('models.connectivity.neverChecked')}</Badge>
  }
  if (status === 'unavailable') {
    return <Badge variant="outline">{t('models.unavailable')}</Badge>
  }
  return (
    <Badge variant={isFailingConnectivity(row.connectivity) ? 'destructive' : 'outline'}>
      {t(`models.connectivity.${status}`)}
    </Badge>
  )
}

function QuotaBadge({ quota }: { quota: QuotaDisplayState }) {
  const { t } = useTranslation('settings')

  if (quota.status === 'unavailable') {
    return <Badge variant="outline">{t('models.unavailable')}</Badge>
  }

  const variant =
    quota.status === 'supported'
      ? 'success'
      : quota.status === 'unsupported' || quota.status === 'authRequired'
        ? 'outline'
        : 'destructive'

  return <Badge variant={variant}>{t(`models.quota.${quota.status}`)}</Badge>
}

function formatLatency(row: ModelAssetRow, unavailableLabel: string) {
  if (row.connectivity.status === 'unavailable') {
    return unavailableLabel
  }
  if (row.connectivity.status === 'never_checked' || row.connectivity.latencyMs === undefined) {
    return '-'
  }
  return `${new Intl.NumberFormat().format(row.connectivity.latencyMs)} ms`
}

function formatTimeout(row: ModelAssetRow) {
  if (row.connectivity.status === 'never_checked' || row.connectivity.status === 'unavailable') {
    return '-'
  }
  return `${new Intl.NumberFormat().format(row.connectivity.timeoutMs)} ms`
}

function formatUsage(
  usage: UsageDisplayState,
  period: 'today' | 'monthToDate' | 'allTime',
  unavailableLabel: string,
) {
  if (usage.status === 'unavailable') {
    return unavailableLabel
  }
  return formatTokenTotal(usage[period])
}

function formatTokenTotal(usage: {
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheWriteTokens: number
}) {
  const total =
    usage.inputTokens + usage.outputTokens + usage.cacheReadTokens + usage.cacheWriteTokens
  return `${new Intl.NumberFormat().format(total)} tokens`
}
