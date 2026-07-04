import { Gauge, RefreshCw, Settings2, Star, Wifi } from 'lucide-react'
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
  isAnySetDefaultPending: boolean
  isProbePending: (configId: string) => boolean
  isQuotaRefreshPending: (configId: string) => boolean
  isSetDefaultPending: (configId: string) => boolean
  onDetails: (configId: string) => void
  onProbe: (configId: string) => void
  onRefreshQuota: (configId: string) => void
  onSetDefault: (row: ModelAssetRow) => void
  rows: ModelAssetRow[]
}

export function ModelMatrix({
  isAnySetDefaultPending,
  isProbePending,
  isQuotaRefreshPending,
  isSetDefaultPending,
  onDetails,
  onProbe,
  onRefreshQuota,
  onSetDefault,
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
      <section aria-label={t('models.matrix.label')} className="model-matrix-layout min-w-0">
        <div className="model-matrix-table-wrap">
          <table className="w-full min-w-[1040px] border-separate border-spacing-0 text-left text-sm">
            <thead>
              <tr className="text-muted-foreground text-xs">
                <HeaderCell>{t('models.columns.identity')}</HeaderCell>
                <HeaderCell>{t('models.columns.provider')}</HeaderCell>
                <HeaderCell>{t('models.columns.default')}</HeaderCell>
                <HeaderCell>{t('models.columns.connectivity')}</HeaderCell>
                <MetricHeaderCell
                  label={t('models.columns.latency')}
                  unit={t('models.units.milliseconds')}
                />
                <MetricHeaderCell
                  label={t('models.columns.timeout')}
                  unit={t('models.units.milliseconds')}
                />
                <MetricHeaderCell
                  className="max-lg:hidden"
                  label={t('models.columns.todayUsage')}
                  unit={t('models.units.tokens')}
                />
                <MetricHeaderCell
                  className="max-lg:hidden"
                  label={t('models.columns.monthUsage')}
                  unit={t('models.units.tokens')}
                />
                <MetricHeaderCell
                  className="max-lg:hidden"
                  label={t('models.columns.totalUsage')}
                  unit={t('models.units.tokens')}
                />
                <HeaderCell>{t('models.columns.quota')}</HeaderCell>
                <HeaderCell>{t('models.columns.actions')}</HeaderCell>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const probePending = isProbePending(row.configId)
                const quotaPending = isQuotaRefreshPending(row.configId)
                const defaultPending = isSetDefaultPending(row.configId)

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
                    <BodyCell>
                      {formatLatency(
                        row,
                        t('models.summary.loadingMetric'),
                        t('models.unavailable'),
                      )}
                    </BodyCell>
                    <BodyCell>{formatTimeout(row, t('models.summary.loadingMetric'))}</BodyCell>
                    <BodyCell className="max-lg:hidden">
                      {formatUsage(
                        row.usage,
                        'today',
                        t('models.summary.loadingMetric'),
                        t('models.unavailable'),
                      )}
                    </BodyCell>
                    <BodyCell className="max-lg:hidden">
                      {formatUsage(
                        row.usage,
                        'monthToDate',
                        t('models.summary.loadingMetric'),
                        t('models.unavailable'),
                      )}
                    </BodyCell>
                    <BodyCell className="max-lg:hidden">
                      {formatUsage(
                        row.usage,
                        'allTime',
                        t('models.summary.loadingMetric'),
                        t('models.unavailable'),
                      )}
                    </BodyCell>
                    <BodyCell>
                      <QuotaBadge quota={row.quota} />
                    </BodyCell>
                    <BodyCell className="rounded-r-md border-r">
                      <div className="flex items-center gap-1.5">
                        <ConfigureButton
                          displayName={row.displayName}
                          onDetails={() => onDetails(row.configId)}
                        />
                        <DefaultButton
                          displayName={row.displayName}
                          isDisabled={isAnySetDefaultPending}
                          isDefault={row.isDefault}
                          isPending={defaultPending}
                          onSetDefault={() => onSetDefault(row)}
                        />
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
        </div>

        <ul className="model-matrix-card-list gap-3">
          {rows.map((row) => {
            const probePending = isProbePending(row.configId)
            const quotaPending = isQuotaRefreshPending(row.configId)
            const defaultPending = isSetDefaultPending(row.configId)

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
                    <ConfigureButton
                      displayName={row.displayName}
                      onDetails={() => onDetails(row.configId)}
                    />
                    <DefaultButton
                      displayName={row.displayName}
                      isDisabled={isAnySetDefaultPending}
                      isDefault={row.isDefault}
                      isPending={defaultPending}
                      onSetDefault={() => onSetDefault(row)}
                    />
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
                  <CompactMetric
                    label={`${t('models.columns.timeout')} ${t('models.units.milliseconds')}`}
                  >
                    {formatTimeout(row, t('models.summary.loadingMetric'))}
                  </CompactMetric>
                  <CompactMetric
                    label={`${t('models.columns.latency')} ${t('models.units.milliseconds')}`}
                  >
                    {formatLatency(row, t('models.summary.loadingMetric'), t('models.unavailable'))}
                  </CompactMetric>
                  <CompactMetric label={t('models.columns.quota')}>
                    <QuotaBadge quota={row.quota} />
                  </CompactMetric>
                  <CompactMetric
                    label={`${t('models.columns.todayUsage')} ${t('models.units.tokens')}`}
                  >
                    {formatUsage(
                      row.usage,
                      'today',
                      t('models.summary.loadingMetric'),
                      t('models.unavailable'),
                    )}
                  </CompactMetric>
                  <CompactMetric
                    label={`${t('models.columns.monthUsage')} ${t('models.units.tokens')}`}
                  >
                    {formatUsage(
                      row.usage,
                      'monthToDate',
                      t('models.summary.loadingMetric'),
                      t('models.unavailable'),
                    )}
                  </CompactMetric>
                  <CompactMetric
                    label={`${t('models.columns.totalUsage')} ${t('models.units.tokens')}`}
                  >
                    {formatUsage(
                      row.usage,
                      'allTime',
                      t('models.summary.loadingMetric'),
                      t('models.unavailable'),
                    )}
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

function ConfigureButton({
  displayName,
  onDetails,
}: {
  displayName: string
  onDetails: () => void
}) {
  const { t } = useTranslation('settings')

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={t('models.actions.configure', { name: displayName })}
          onClick={onDetails}
          size="icon"
          type="button"
          variant="outline"
        >
          <Settings2 aria-hidden="true" className="size-4" data-icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{t('models.actions.configureShort')}</TooltipContent>
    </Tooltip>
  )
}

function DefaultButton({
  displayName,
  isDisabled,
  isDefault,
  isPending,
  onSetDefault,
}: {
  displayName: string
  isDisabled: boolean
  isDefault: boolean
  isPending: boolean
  onSetDefault: () => void
}) {
  const { t } = useTranslation('settings')

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={
            isPending
              ? t('models.actions.settingDefault', { name: displayName })
              : isDefault
                ? t('models.actions.currentDefault', { name: displayName })
                : t('models.actions.setDefault', { name: displayName })
          }
          disabled={isDefault || isDisabled}
          onClick={onSetDefault}
          size="icon"
          type="button"
          variant="outline"
        >
          <Star aria-hidden="true" className="size-4" data-icon />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {isPending
          ? t('models.actions.settingDefaultShort')
          : isDefault
            ? t('models.actions.currentDefaultShort')
            : t('models.actions.setDefaultShort')}
      </TooltipContent>
    </Tooltip>
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

function MetricHeaderCell({
  className,
  label,
  unit,
}: {
  className?: string
  label: string
  unit: string
}) {
  return (
    <th
      aria-label={`${label} ${unit}`}
      className={`border-border border-b px-3 py-2 font-medium ${className ?? ''}`}
    >
      <span className="grid gap-0.5">
        <span>{label}</span>
        <span className="font-normal text-[11px]">{unit}</span>
      </span>
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

  if (status === 'loading') {
    return <Badge variant="outline">{t('models.summary.loadingMetric')}</Badge>
  }
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

  if (quota.status === 'loading') {
    return <Badge variant="outline">{t('models.summary.loadingMetric')}</Badge>
  }

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

function formatLatency(row: ModelAssetRow, loadingLabel: string, unavailableLabel: string) {
  if (row.connectivity.status === 'loading') {
    return loadingLabel
  }
  if (row.connectivity.status === 'unavailable') {
    return unavailableLabel
  }
  if (row.connectivity.status === 'never_checked' || row.connectivity.latencyMs === undefined) {
    return '-'
  }
  return new Intl.NumberFormat().format(row.connectivity.latencyMs)
}

function formatTimeout(row: ModelAssetRow, loadingLabel: string) {
  if (row.connectivity.status === 'loading') {
    return loadingLabel
  }
  if (row.connectivity.status === 'never_checked' || row.connectivity.status === 'unavailable') {
    return '-'
  }
  return new Intl.NumberFormat().format(row.connectivity.timeoutMs)
}

function formatUsage(
  usage: UsageDisplayState,
  period: 'today' | 'monthToDate' | 'allTime',
  loadingLabel: string,
  unavailableLabel: string,
) {
  if (usage.status === 'loading') {
    return loadingLabel
  }
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
  return new Intl.NumberFormat().format(total)
}
